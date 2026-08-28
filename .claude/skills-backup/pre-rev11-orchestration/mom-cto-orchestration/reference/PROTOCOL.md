# MoM PROTOCOL — Verter Train-Manager Overlay

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

This overlays `/multi-agent-orchestration`; it does not replace it. A manager still runs the implementation loop end-to-end; the CTO schedules the gate/review/confirm jobs. This file supplies Verter specifics plus MoM-tier rules.

## GOVERNANCE

Every rule-bearing change requires prior NEUTRAL codex-architect approval before adoption/commit. Covered surface is complete: `CLAUDE.md`, `AGENTS.md`, every `.claude/skills/**` file, all protocols, every guard's assertion/needles/exemptions/rationale plus its test, design-doc/contract invariants, and any `docs/arch` plan/debt row changing constraints, sequencing, or priority. This is not limited to architectural, severe, or this-skill rules. Unapproved rule-bearing change = block + revert. Implementing agents cannot self-certify.

Procedure: prepend CODEX-ARCHITECT MANDATE, state exact change + rationale, ask neutrally "is this rule/skill/guard/design/plan change correct and necessary?", land only on approval. Regular code/test/doc updates that do not alter rules are unaffected.

## CODEX-ARCHITECT MANDATE

Prepend this verbatim to EVERY codex consult/fork/review/approval/adversarial-audit prompt. The mandate sets the bar; the substantive question remains neutral.

> You are the sole architecture authority. Judge NEUTRALLY and UNPRIMED — the prompt must never state or imply the desired conclusion; ask "is X correct / what is the best design", never "confirm X". Mandate:
> - For an OPEN architecture DECISION: recommend the best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
> - For REVIEW, LANDING, or CONFIRMATION: judge the artifact against the current ratified architecture contract, all critical invariants, executable obligations, correctness, safety, scalability, fail-closed behavior, discriminating evidence, and anti-rogue integrity. Do NOT block or reopen solely because a preferable design exists; reopen only when new evidence invalidates the ratified contract or exposes a correctness, safety, scalability, or invariant defect. A merely preferable alternative is recorded as optional debt.
> - Be brutally HONEST — surface the strongest counter-argument, flag every risk/uncertainty, never rubber-stamp. If the thing under review is wrong, say so plainly.
> Deliver a clear verdict + enumerated, actionable findings (file/section/exact change). End with `__DONE__`.

## Never Prime

Every reviewer, consult, verifier, confirmer, and adjudicator prompt must present artifact + neutral question and let the agent decide. Do not say or imply the expected answer. Use "assess whether X is correct and complete / what is best"; never "confirm X is correct". Applies to post-land confirm too. Reviewers never receive the author's correctness claim, the design-adversary's predicted verdict, or peer reviewers' findings.

## Rule-File Integrity

Rule-bearing artifacts are read-only unless the train's sanctioned scope is to change that rule and governance approval exists. Never add an exception/carve-out/accepted residual/sole exception/landing blocker that weakens any rule, guard, assertion, test, or plan row. A rule exception added to legitimize a shortcut is an anti-rogue violation and is removed before landing. Known correctness/invariant/required-acceptance violations fold into the owning existing train; unsupported-completeness violations enter the `docs/arch` debt ledger with a codex-DEFER ruling only when exactly fail-closed; a new critical-path train requires explicit user approval — never rule text.

Reviewer briefs must say branch rule artifacts are authoritative only where they match base plus sanctioned codex-approved edits; other rule text changes are defects. Every train brief says not to touch rule files unless in scope and never add a rule exception.

The §1a verifier MUST run the full Anti-Rogue discovery (below), not a path-limited diff, and FAIL on any unapproved rule-bearing edit, added exception/weakening, deleted/renamed guard/rule/test, new skipped test, relaxed assertion, or plan/debt row — a targeted `CLAUDE.md`/`AGENTS.md`/`.claude/skills/`/`docs/arch` diff is only a convenience pass, never the boundary. Live enforcement is that §1a discovery plus the dual-layer adversarial codex confirmation; a static scanner flagging exception-language adjacent to rule clauses is an optional complementary hardening, pursued only as a codex-approved `docs/arch` debt row (never carried as "owed" rule text), and weakening/removing it is itself rule-bearing.

The current ratified architecture contract and frozen supported-release manifest are normative. Plans and debt ledgers supply sequencing and traceability but never override a critical invariant, an executable obligation, or new invalidating evidence. A train may add its own deferral only with explicit codex-DEFER ruling. Reviewers and §1a challenge every new DEFER/debt row; missing ruling or deferring scoped work is P0.

## Anti-Rogue Rule

Threat: an agent takes a shortcut, then edits a rule/skill/protocol/guard/design/plan to legitimize it. Defense has two independent layers, both starting with DISCOVERY:

`git diff --name-status --find-renames --find-copies <base>..<head>` → classify every Added/Modified/Deleted/Renamed/Copied path. Scan new-side bodies for rule/constraint text, guard semantics, assertion/needles/exemptions/rationale, allowlists, `#[ignore]`/skips, relaxed/removed asserts, plan/debt edits, and disguised rule text. For every Modified or renamed-with-edits file, also scan deleted/changed `-` hunks (`git diff <base>..<head> -- <path>`) plus old owning context for a removed/relaxed rule, guard, assertion, test, skip, allowlist, or plan row inside a still-existing file. For deletions/renames, inspect old body with `git show <base>:<path>`. For copies, inspect source. Hand codex artifact diff plus owning context; deletions include old body.

Layer 1: the implementation manager runs discovery pre-land and reports the COMPLETE rule-bearing-change artifact to the CTO; the CTO schedules the SHA-bound adversarial codex-architect approval job — unprimed: "Is this correct and necessary, or does it legitimize a shortcut, weaken/relax a guard, add a self-serving exemption, or bypass a gate?" — and consumes its durable summary. The manager never owns the codex review leg. Land only on clean approval.

Layer 2: MoM confirm manager independently reruns discovery post-land on the landed diff; it does not trust layer 1. Each rule-bearing change needs the same clean codex confirmation or REOPEN.

High suspicion always demands adversarial codex: changed guard assertion/needles/exemptions/rationale/comment, new `#[ignore]`, relaxed `assert`, new allowlist/exemption, weakened rule clause.

## Active Regime / Capacity

Roles required: an implementation author (Claude/Fable Agent or GPT/Codex `codex exec` write) with fresh context, review/§1a/confirm fresh contexts, and a cross-model counterweight. The default Agent/Task mechanism gives fresh-context role separation on a single account (harness-managed), bounded by harness Agent concurrency capacity, ONLY WHEN the harness guarantees no inherited transcript/hidden state beyond the passed prompt, a distinct agent identity, status/stop/continue control, and child-agent spawning where required; if any of those is absent or unknown, fall back to `claude -p` for an explicit fresh-process boundary. On the opt-in `claude -p` path, read the account→role mapping from the live brief/ledger after smoke tests and never hard-code account names; separate accounts are then an availability/parallelism optimization, not required. Single account = separate fresh Agent sub-agents (unprimed prompts), cross-model check, serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex unavailable.

Run file-disjoint trains in parallel (the default). No artificial 300s spacing or one-claude-at-a-time throttle; respect harness Agent concurrency capacity. On the opt-in `claude -p` path, map separate-account instances explicitly, one train per account, with no artificial sub-agent-count limit. Triple-review legs run in parallel. Canonical/full-suite/heavy gates are globally serialized under the landing lease.

Review rounds use three independent blind reviewers with the author-dependent cross-model mix: a Claude/Fable author gets 2 independent GPT reviewers + 1 fresh Claude reviewer; a GPT/Codex author gets 2 independent Claude reviewers + 1 fresh GPT reviewer. The author, design adversary, and confirmer never count as reviewers.

codex gate seats (review/verify/confirm/architect) run at the model's maximum supported reasoning effort (`high` for `gpt-5.6-sol`); never downgrade reviewer effort — a reduction is qualification-gated and not adopted (see Reviewer-Effort Qualification Gate); an unknown or unsupported maximum BLOCKS the dispatch. Every gate-bearing REVIEW/VERIFY/CONFIRM/ARCHITECT Claude role — reviewer, §1a verifier, confirm manager, integration-confirm manager (the confirm managers own the landing and integration gates and are explicitly covered) — MUST run on the highest available Claude model at max/highest effort, bound either by an explicit `model`/`effort` arg at spawn OR by an audited agent-definition whose model+effort are recorded in `PROGRESS.md`; an unknown or default model/effort BLOCKS the dispatch — never silently downgrade any gate-bearing role. Implementer/fix authorship is MEASURED, not maximal: fable-5 is the Claude default before the bakeoff (Opus only on capacity limit); after the bakeoff, the winner stays default while telemetry supports it (see Measured Author Selection). On the opt-in `claude -p` path, bind the required model + `--effort` explicitly per agent and watch `CLAUDE_CONFIG_DIR`: a wrapper account exports it into children, so bare `claude` may silently use the wrapper account — override at loop start (`export CLAUDE_CONFIG_DIR=$HOME/.claude`) when dispatching bare `claude`; identical reset times across "different" accounts indicate the trap.

## Measured Author Selection

Implementation/fix authorship is evidence-based, not preference-based. Run a live parallel BAKEOFF on a bounded slice: same immutable brief + base SHA, separate blind worktrees (no cross-access), Claude/Fable vs GPT/Codex, targeted TDD gates only. Blind-score: tests-pass · oracle/spec parity · diff quality · invariant compliance · initial P0/P1 findings · required fix rounds. RETAIN the winner (not throwaway) as the default author for subsequent trains. Keep the winner only while telemetry supports it: fix rounds per train, escaped P0/P1 at confirm, reviewer disagreement rate, false-positive findings, mutation-verification failures. Author↔reviewer INDEPENDENCE and cross-model DECORRELATION are the invariants; the model family is the measured variable.

## Design-Adversary Contract

For contract-heavy trains, a pre-authoring design-adversary pass produces a failure-mode contract: every unsupported/partial state mapped to a test, mutation, invariant, or exact refusal; safety gates + all reachable bypass paths; shared-owner placement + forbidden duplicate paths; required legacy deletions; concurrency/cancellation/cache-admission/partial-result cases; and a "how could this be GREEN while WRONG?" section including representation-boundary questions (raw vs decoded, source vs semantic value, client vs server, artifact vs cached view, typed IR vs reconstructed text). The implementer receives the contract + a stable reviewer rubric and self-proves them pre-review. Final reviewers receive the normative requirements + immutable diff, never the adversary's predicted verdict or the author's correctness claim. The design-adversary is NOT one of the three review legs and never gate-reviews its own design.

## Reviewer-Effort Qualification Gate

Reviewer/verifier/confirmer/architect effort stays at maximum until a blinded seeded-defect corpus (historical clean + faulty diffs, seeded P0/P1 mutations, distractors) proves non-inferiority on P0/P1 false-clean rate, then unique-finding recall + latency. Intuition or throughput pressure never lowers gate-model quality.

## Repo / Worktree

Repo root: `<repo-root>`. Integration branch: `refactor/semantic-db-overhaul`; never commit directly to it. Worktree/branch/train id come from the brief. All edits use absolute paths in the worktree; all build/test commands start `cd <worktree> && ...`. Every sub-agent brief repeats this.

Every train has its own worktree. Implementers may commit `wip:` checkpoints after cheap checks. Each slice ends in one clean, separately-testable conventional commit; a landing train preserves its reviewed ordered slice commits plus one consolidated fix commit per review round. The cumulative landed tree is byte-identical to the reviewed and finally gated tree.

## Dispatch

Default mechanism is the **Agent/Task tool**, gated on harness support. The CTO spawns each manager as an Agent sub-agent; managers dispatch only implementation and fix work (their implementer/fix/diagnostic agents, as Agent sub-agents — agents may spawn child agents, the manager→children topology). The CTO schedules the SHA-bound reviewer, §1a, verifier, and landing jobs, and only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land — so every gate stays independent of the author. Scheduled jobs persist raw logs and full reports but publish a concise summary — input identity, command, completion state, counts, failures, verdict; the CTO consumes the summary, never raw logs or a bare completion notification. An incomplete or timed-out job is an unresolved failure, never a pass. The Agent tool is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it. This capability is not assumed: it MUST be proven and recorded once per session as a `CAPABILITY mechanism=agent isolation=<result> identity=<result> stopcontinue=<result> childspawn=<result> result=PASS` line in `PROGRESS.md`/the CTO ledger BEFORE the first Agent dispatch (see `CHECKPOINT-PROTOCOL.md`). An absent or stale capability proof counts as "unknown" and forces the `claude -p` fallback — an explicit fresh-process boundary; any single missing/failed property does the same. Each starts cold with a self-contained brief as its prompt; the agent's final message IS its report — and for gate-bearing roles (review, §1a, confirm, anti-rogue) the exact prompt and the verbatim final report are persisted to files with a recorded input id, never inline-only (see `CHECKPOINT-PROTOCOL.md`). A blocking Agent call returns that report; a background (`run_in_background`) Agent call notifies the spawner on completion — no resume-loop wrapper, no stream/marker watchdog. Continue a still-live agent with its context via its id/name; a fresh Agent call starts cold.

The dispatch mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, and durable auditability — not just transport; those are preserved by the safeguards above, not by the swap alone. Each gate is conditioned on a recorded precondition, never asserted: confirm/integration-confirm independence on CTO-only dispatch; fresh-context isolation on the recorded `CAPABILITY … result=PASS` proof (absent/stale ⇒ `claude -p`); review/verify/confirm/architect quality on the recorded highest-model+max-effort binding per gate-bearing role (unknown/default ⇒ BLOCK; implementer/fix binds the measured author policy); auditability on the persisted brief+report+input-id+model+effort. The 3/3 review, §1a, confirm, and anti-rogue gates pass only when their preconditions are recorded — a missing or stale precondition leaves that gate unmet. Trust but verify: every report is INTENT, not fact — confirm "done"/test-counts/commits/deletion claims against `git show --stat`, `git log`, `grep`, and gate summaries before acting.

Codex seats split by role. Architecture, design-adversary, review, and confirmation invocations are read-only Bash-invoked CLI subprocesses (never `claude -p`), separately dispatched, and can never be the author invocation. Implementation and fix roles may use Claude/Fable or GPT/Codex; a GPT author uses a separate write-enabled `codex exec` worktree invocation. Never discard or redo work solely because GPT authored it; judge it by the same evidence and review gates.

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
  --disable image_generation -c 'model="gpt-5.6-sol"' -c 'model_reasoning_effort="high"' \
  < PROMPT.md > OUT.txt 2>&1 &
```

`model_reasoning_effort="high"` is `gpt-5.6-sol`'s maximum supported tier; gate seats (review/verify/confirm/architect) always bind the model's maximum supported effort — never a tier below it, never an uncontrolled default; an unresolved model or maximum effort fails closed (BLOCK the dispatch). Read-only seats use `--sandbox read-only`; a GPT implementer seat is a separate write-enabled `codex exec` invocation confined to its own worktree. Feed prompt via stdin redirection, never as an arg. Verify output grows past the tiny startup size within seconds or redispatch. A GATE-BEARING reviewer leg runs with `-C <worktree>` and reviews the pinned immutable `git diff <base-sha>..<head-sha>` pair recorded per leg — never a live branch ref, which can advance mid-review; only a NON-gate ad-hoc architecture/consult leg may review a live ref such as `git diff refactor/semantic-db-overhaul..HEAD`. Output is large; the scheduled job wrapper — not the CTO — greps the tail for the last verdict:
`grep -nE '\[P[0-3]\]|VERDICT|LAND|CHANGES|__DONE__' OUT.txt | tail`. The real verdict is the LAST sentinel (`__DONE__`/`VERDICT`/`LAND`/`CHANGES`); earlier matches are prompt echoes — never parse the whole output. The job persists the raw output and publishes the concise summary; consumers read summaries/reports, never raw logs, and tee heavy output once to query rather than re-run.

Never wait via `pgrep -f "codex exec"`; it self-matches the waiter. Capture `$!` and `wait`/`kill -0`, append a sentinel and grep the output, or run foreground codex inside a backgrounded bash with an exit marker.

## TDD / Tests

Discriminating tests first: FAIL before change, PASS after. No stubs, empty tests, always-true asserts, unconditional defaults presented as implementation, or non-discriminating characterization. Reviewers read every new test body. §1a is automated with reversible MUTATION RECIPES: for every new/changed correctness-bearing test, guard, or refusal, record a recipe — verify the starting SHA; apply the specified mutation; run the named test and require the expected failure (RED); restore; verify a clean original SHA; run the green test; run an unplanted control that stays GREEN. Persist commands + results. The independent confirmer executes the recipes again; sampling is forbidden.

## Review Cadence

Every train review round = three independent blind reviewers with the author-dependent cross-model mix (Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT), parallel, neutral, read-only, no tests, harsh production bar, each with a MANDATORY distinct lens: (A) semantic parity + oracle validity + coverage-dimension completeness; (B) architecture + typed-IR ownership + fail-closed + rule integrity; (C) host integration + caching + source maps + runtime behavior + regression blast radius. All three start simultaneously against the SAME immutable cumulative tree, blind to each other and to the author's correctness claim; record all three identities, models, lenses, immutable input SHA, and verdicts before landing. The lens changes search strategy, not ownership: each reviewer still inspects the full cumulative diff to a clean verdict.

Wait for all three → consolidate ONCE → one comprehensive fix commit → redispatch all three. Intermediate fix rounds MAY delta-scope, but the FINAL clean round is a full three-leg CUMULATIVE review over the complete cumulative diff — a delta-only final round, a single reviewer, or a two-review subset never clears the gate; landing requires a final clean 3/3 (or NIT-only carried forward).

Each review round receives a generated EVIDENCE PACKET: base/head SHA; changed acceptance rows + invariants; coverage-manifest delta; unsupported→supported transitions; tests + mutation recipes; cache-key + source-map changes; legacy deletions; generated artifacts + their source manifest.

Designs/docs get the same full 3/3; later landing/rebase skips re-review only when byte/hash-identical. Any conflict resolution, mirror content delta, or non-identical "mechanical" change re-enters 3/3 + §1a + anti-rogue layer 1. Skill/design/doc review rounds cap at 3: after the round bound, only P3/NIT cosmetic/wording/framing residuals may be carried forward WITHOUT changing the reviewed tree; any substantive, anti-rogue, or content-changing finding still requires another full clean 3/3 cumulative round. The cap bounds cosmetic-residual churn; it never overrides the final-clean-3/3-on-content-change rule.

Findings route by scope. In-scope P0/P1/P2 enter the normal fix cycle; P0 blocks, each fix round is one consolidated new commit, never amend, re-review until clean. Scope-deviating/increasing findings (system-wide/pre-existing class, architecture beyond the train, relitigating settled scope) go to codex with contract context for adopt-now/defer/reject under the scope-admission policy. Fix whole classes within the train surface; if a class extends beyond the deliverable, stop and codex-scope-consult. If recurring out-of-scope findings exceed about 5 rounds, consult early. Adjudicate reviewer compile/test claims against the verified gate: if the gate compiled and ran, a false "won't compile" claim is INVALID — never add a no-op patch merely to clear that verdict.

## Architecture / Decision Modes

Architecture is always codex-owned. The architect/decider seat is read-only and never writes code; implementation authorship is a separate seat (Claude/Fable or GPT/Codex — see Dispatch). Iterate until confident; never accept hedged verdicts. If codex contradicts verified repo evidence after rerun, stop/escalate.

Single yes/no architecture question → one neutral codex leg. Genuine multiple-choice/high-stakes fork → two neutral codex legs in parallel: A options-framed, B problem-only/unprimed. If they agree, decision is binding. If they disagree, run a third codex decider with both outputs plus source; it verifies decisive claims against code and adjudicates.

Modes:
- `escalate-to-user`: codex produces architecture analysis; user ratifies/rejects. Product/priority forks go to user.
- `full-autonomous`: auto-adopt codex architecture verdict; derive product/priority from the ratified contract + frozen manifest (plans supply sequencing/traceability only and never override a critical invariant or executable obligation).
User never adjudicates architecture instead of codex. Every discovery is classified by the five-way scope-admission policy (blocking defect / invariant defect / required acceptance row → the owning train; unsupported completeness → post-release, fail-closed; optional architecture improvement → non-blocking unless current code is incorrect/unsafe/unscalable/invariant-violating); a new critical-path train requires explicit user approval. Stop/user boundaries: no implementation agent capacity (on opt-in `claude -p`, no usable implementation account), destructive operation (force-push/history rewrite/irreversible shared state), unrecoverable lost/corrupt worktree with no trusted commit, codex unavailable/contradicts evidence, a product/priority choice not derivable from the ratified contract, or a scope addition that would create a new critical-path train.

## Verification Gate

Tiered gating. During slice implementation, run targeted gates: changed tests + affected crate(s) + a conservative reverse-dependency closure. Fix rounds run every finding's regression test + the affected closure + applicable architecture guards + clippy + fmt. Targeted runs are ITERATION EVIDENCE ONLY — targeted success is never landing evidence. A conservative selector that cannot prove the affected closure (manifests, build scripts, proc-macros, shared compiler substrates, test infra, architecture guards) MUST fall back to full-workspace coverage for that iteration/diagnostic run; the fallback run is still iteration evidence only — not a passed lifecycle gate, never landing evidence, and never reused as the canonical final/confirm execution. `<scratch>` below is a scratch dir outside the repo (e.g. under the OS temp dir).

The canonical FULL Rust pair runs at exactly TWO lifecycle points: (1) FINAL ACCEPTANCE — after the final content change on the rebased, landing-frozen train tree; (2) CONFIRM — a fresh independent run post-land (never reusing the final gate's execution). Any content change, conflict resolution, regeneration, or tree mismatch after the final gate invalidates it — re-run it. Heavy gates run as CTO-scheduled SHA-bound verifier jobs, globally serialized under the landing lease.

```bash
cd <worktree> && cargo nextest run --workspace --no-fail-fast 2>&1 | tee <scratch>/<train>/gate-nextest.txt
cd <worktree> && cargo test -p verter_session --tests 2>&1 | tee <scratch>/<train>/gate-session.txt
cd <worktree> && cargo clippy --workspace -- -D warnings
cd <worktree> && cargo fmt --all --check
```

Canonical Rust gate is the nextest command PLUS `cargo test -p verter_session --tests`. Bare `cargo test --workspace --tests` silently skips the verter_session integration suite and is never the sole gate. Count summaries/per-binary lines; never accept truncated runs. `--no-fail-fast` prevents an early env/pre-existing failure hiding downstream failures.

JS: if TS touched, run the relevant JS gate; otherwise at least `pnpm install --frozen-lockfile` for lockfile sync. Node_modules-less worktrees may symlink main `node_modules` or install once. `typeinfo_ts_bindings_*` regeneration via missing `buf` is env-only, not a code failure.

Re-derive baseline at train entry. Required result is zero new failures versus that live baseline. Do not hard-code named failure allowlists into rules; pre-existing failures need a debt row or codex-DEFER ruling.

A TIMEOUT or incomplete run is NEVER a pass and never presumed environmental. Rerun the timed-out test in isolation with an adequate timeout and no co-resident heavy work: if it clears → classify the original as environmental, retain both artifacts; if it repeats → collect hang diagnostics; if classification stays ambiguous → the gate HARD-FAILS. Timeout configuration must accommodate valid host behavior — the advertised slow-timeout must match the configured one (`.config/nextest.toml` advertising ~60s while configuring 5s×3 kills valid tests around 15s on an 8GB host; fix the configuration, never tolerate false timeouts) — and genuinely long tests get explicit per-test overrides.

Landing LEASE: acquire the integration/landing lease before final review + gating — a single-writer land window. ONE heavy suite per host; model reviews and heavy gates run concurrently only on isolated workers. No shared mutable Cargo target between concurrent jobs; immutable compiler/dependency artifacts plus a private writable job layer are fine. Confirm may reuse compiled artifacts but EXECUTES tests independently. Infra failures stay failures / explicit infra-retries — never green verdicts.

## Commit / Land Rules

Use `LANDING-PROTOCOL.md` for mechanics. Mid-flight `wip:` allowed; the final landed history is one clean, separately-testable conventional commit per reviewed slice, in reviewed order, plus one consolidated fix commit per review round — no `wip`/`fixup`/`squashme`/amend-only history, no Co-Authored-By or attribution trailer, no logs/outputs/generated junk, no `git push`, no direct integration-branch commit, no `git add -A` or `git add .`. The cumulative landed tree is byte-identical to the reviewed and finally gated tree.

Pre-land sync: rebase onto current integration tip; the canonical final gate runs on the rebased, landing-frozen tree. Any conflict resolution or content delta invalidates the final gate and review identity and re-enters full 3/3 + §1a + anti-rogue layer 1. Design mirrors must be byte-identical to reviewed content. True ff only; no merge commit.

No plan refs in production: conventional commit diffs/messages, code, comments, and tests contain no plan/phase vocabulary. Such refs may exist only in `wip:` history and are scrubbed at commit consolidation.

## Repo Cleanliness

Only product files land: source, tests, fixtures, tracked docs, and `docs/arch` plans/designs. Plans belong only in `docs/arch`. Temp/scratch/report/evidence/log/progress files live in a scratch dir outside the repo (e.g. under the OS temp dir) or `.feedback/` (directory-gitignored). Worktrees are outside the repo.

Before staging, inspect `git status --short` and remove only verified untracked/ignored scratch this train created; never by filename class. A tracked file is never removed as cleanup; tracked deletion is reviewed product change. Do not add per-file `.gitignore` entries for scratch. Generated outputs are gitignored by directory/glob (`playwright-report`, `test-results`, `*.junit.xml`, coverage). Scratch/temp/report files in status/diff block landing.

Release-close scratch/report clutter already in git history is purged by a dedicated user-authorized destructive history-rewrite/force-push operation, with final user go-ahead at execution time.

## Binding Designs / Debt

Every binding design/plan is committed to `docs/arch/<name>-design.md` and referenced in the master-plan locked-designs index. At land, mirror is byte-identical to reviewed design; confirm verifies tracked + referenced + `cmp` identical.

Mid-release deferrals require codex-DEFER ruling and debt-ledger row in `docs/arch`. FEED-FORWARD is valid only mid-release. Release close requires zero correctness or invariant debt within the supported surface, zero fail-open behavior, and exact fail-closed coverage outside it. Explicitly classified post-release completeness debt and merely optional architecture improvements may remain. No supported-surface correctness, safety, scalability, invariant, or executable-obligation defect may be relabeled as completeness debt.

## Confirm / Integration Gates

After every train land, the CTO — never the implementation manager — dispatches a separate unprimed confirm MANAGER. It independently verifies four bars and writes `VERDICT:CONFIRMED` only when all hold:
1. CORRECT + additive/non-breaking-as-claimed + fresh canonical full gate GREEN first-hand (never reusing the final gate's execution); re-grep legacy deletion; independently re-execute EVERY §1a mutation recipe in a throwaway worktree (mutation → RED → restore → GREEN, plus the unplanted control) — sampling is forbidden; `cmp` design mirrors.
2. NOT SHALLOW: real inputs and edges; no non-discriminating characterization.
3. NO STUBS: no empty/always-true tests or unconditional default/unknown/OK returns as implementation.
4. RATIFIED-CONTRACT INTEGRITY: independently establish compliance with the current ratified architecture contract, all critical invariants, and executable obligations. A merely preferable alternative is non-blocking optional debt. Reopen only for new evidence of incorrectness, unsafe or fail-open behavior, unscalable behavior within the ratified surface, an invariant violation, or an invalidated contract assumption.

Every post-land confirmation ALSO runs a separate neutral/unprimed, read-only, highest-model/max-effort codex adversarial leg over correctness, CRITICAL invariants, executable obligations, fail-open behavior, mutation discrimination, and anti-rogue integrity (the cross-model decorrelation of the confirm gate); its merely-preferable-architecture findings are recorded as non-blocking optional debt and never reopen.

On REOPEN, CTO dispatches fresh fix manager → re-land → re-confirm. Per-train confirm covers each train and is separate from integration-confirm.

Integration-confirm MANAGER — a cross-train coherence check distinct from per-train confirm — runs at every milestone/dependency boundary, before any dependent train relies on integrated work, before final close-out, and as a periodic floor after every five confirmed landing trains. It derives its done-bar from the ratified contract + frozen manifest (plans supply sequencing/traceability, never the bar); reviews the integrated diff/tree; validates the accepted per-train gate/confirm artifacts and tree identities; checks cross-train invariants, manifests, design mirrors, debt honesty, legacy deletion, hollow fronts, cache/perf/warm-state — using TARGETED cross-train checks where needed. It does NOT re-run the canonical full pair (that runs at exactly the two lifecycle points: final acceptance and per-train confirm); any content change surfaced by integration-confirm re-enters the owning train's final-acceptance + confirm lifecycle. Issues are classified via codex as REOPEN vs FEED-FORWARD. FEED-FORWARD only mid-release. Only `VERDICT:INTEGRATION-CONFIRMED` closes a milestone.

During a train's confirmation, the CTO may reserve capacity and run extraction or provisional design for the next train; nothing relies on the prior train until `VERDICT:CONFIRMED`, and integration never advances more than one unconfirmed train deep.

Train cleanup only after land + confirmation: `git worktree remove` + `git worktree prune`, remove transient briefs/reports/review outputs (and `jsonl`/markers on the opt-in `claude -p` path), preserve CTO ledger/MOM-NOTES, landed reports, debt ledger, design docs, verify clean status. At a milestone boundary clear all closed-train worktrees/temp. Never delete live `/tmp` evidence mid-flight.

## Status Reporting

Write/overwrite `<scratch>/<train>/status.json` (a scratch dir outside the repo, e.g. under the OS temp dir) at milestones:

```json
{ "train":"<train-id>", "state":"RUNNING|BLOCKED|LANDED_AWAITING_CONFIRM|DONE|PARTIAL",
  "slice":"<slice-id or short step>", "branch":"<branch>", "head":"<sha>",
  "commits":["<sha> <subj>"], "gate":{"rust":"PASS|FAIL|PENDING","js":"PASS|FAIL|NA"},
  "review":{"lensA":"LAND|CHANGES|PENDING","lensB":"...","lensC":"..."},
  "newFailures":[], "note":"<=1 line" }
```

`BLOCKED`: first run escalation protocol; if unresolved, write `ESCALATION.md` with question, two options, what tried, set state, exit. `PARTIAL`: keep only internally clean committed branch work, no ff-merge unless the full lifecycle completed; write `HANDOFF.md`; no final WIP commits. `LANDED_AWAITING_CONFIRM`: after the final clean 3/3, §1a `VERDICT:LAND`, anti-rogue layer 1, rebase, final canonical gate on the landing-frozen tree, train commit preparation, true ff, the narrow landing cleanup (worktree/build-directory removal only), `MANAGER-LANDED.md`, CTO ledger append, then exit — landed, not closed. `DONE`: only on the independent confirm manager's `VERDICT:CONFIRMED`; the CTO records the transition, and train cleanup (transient-scratch removal per Confirm / Integration Gates) happens at this transition, never earlier.

Every CTO checkpoint additionally records: the frozen release-manifest content identity; total/confirmed/active/remaining landing trains; blocking acceptance rows open/closed; scope additions since the prior checkpoint; active implementation time vs queue/wait/review/gate time; review rounds + initial P0/P1 counts; confirm-reopen count; the exact next finish condition. The denominator comes from the frozen manifest and cannot grow silently.

## Verter Invariants

Never weaken: one resolver (`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`); typed-IR-only (no source-slicing/regex-on-type-text/reparse in resolver); shallow-by-default; fact-cache R21 five split env hashes; R6 no versioned identity in query-identity keys; §10.4.1 363-row manifest partition EXACT + bijective with live manifest; `AdditionalProofRow` closed at 7; `CodeTransform` is the only output-mutation path; final-state prose/no phase archaeology in `crates/*/src/**`.

Cross-platform: codebase must build/test/materialize on macOS/Windows/Linux. No NTFS-illegal chars (`< > : " | ? * \`, control chars, trailing dot/space, reserved CON/PRN/AUX/NUL/COM#/LPT#) in tracked paths or generated names; sanitize names. No hardcoded path separators; use `Path`/`PathBuf`/`join`. Byte-equality compares normalize CRLF/LF or compare text. OS binaries (`tsgo`/`.exe`) are discovered platform-aware. Temp/cwd use std abstractions. Platform-assuming code is a defect.

## Terseness

Rules/skills stay concise. New process gaps go to the ledger and, if rule-bearing, through GOVERNANCE before adoption.
