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
> - Prefer the correct, durable design over the easy/fast one, regardless of effort, size, or migration breadth.
> Deliver a clear verdict + enumerated, actionable findings (file/section/exact change). End with `__DONE__`.

## Never Prime

Every reviewer, consult, verifier, confirmer, and adjudicator prompt must present artifact + neutral question and let the agent decide. Do not say or imply the expected answer. Use "assess whether X is correct and complete / what is best"; never "confirm X is correct". Applies to post-land confirm too.

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

## Active Regime / Accounts

Read account→role mapping from the live brief/ledger after smoke tests; never hard-code account names. Roles required: implementer/fix fresh context, claude review/§1a/confirm fresh context, codex. Separate accounts are preferred, not required. Single account = separate fresh `-p` invocations, unprimed prompts, codex cross-model check, serialized heavy gates. Stop/escalate if all implementation accounts are capped/logged out or codex unavailable.

Run file-disjoint blocks in parallel, one block per available account. No 300s spacing, one-claude-at-a-time throttle, or sub-agent-count limit. Triple-review legs run in parallel. Canonical/full-suite/heavy gates are globally serialized under one gate lock.

Bind highest available claude model + `--effort max` explicitly for every claude agent; codex uses highest reasoning (`gpt-5.5`/xhigh here). Never downgrade reviewers. Watch `CLAUDE_CONFIG_DIR`: a wrapper account exports it into children, so bare `claude` may silently use the wrapper account. Override at loop start (`export CLAUDE_CONFIG_DIR=$HOME/.claude`) when dispatching bare `claude`; identical reset times across "different" accounts indicate the trap.

## Repo / Worktree

Repo root: `/Users/carlosrodrigues/Documents/dev/verter`. Integration branch: `refactor/semantic-db-overhaul`; never commit directly to it. Worktree/branch/block id come from the brief. All edits use absolute paths in the worktree; all build/test commands start `cd <worktree> && ...`. Every sub-agent brief repeats this.

Every phase/stage has its own worktree. Implementers may commit `wip:` checkpoints after cheap checks. After CONFIRMED, each landed block squashes to exactly ONE clean conventional commit; a stage/phase may contain multiple block commits only because multiple blocks landed independently. Only conventional commits require the full gate.

## CLI Dispatch

Use CLI subprocesses only; never Task/Agent tools. Each agent gets a self-contained brief file and starts cold.

```
<CLI> -p --model '<model>' --effort max --dangerously-skip-permissions \
  --output-format stream-json --include-partial-messages --verbose \
  < BRIEF.md > OUT.jsonl 2>&1 &
```

Detach resume-loop wrappers (`nohup`/`setsid`) and monitor stream growth plus status/report/marker/git activity. Do NOT kill an idle resume-loop wrapper that is still self-healing; intervene only on a true hang — no activity on ANY signal (stream/status/git/markers) across multiple bounded windows — by killing both the wrapper and the inner `claude -p` (`pkill -f BRIEF` misses the inner, which reads stdin), confirming `ps` count 0, then redispatching. Conversely a LIVE agent with repeated activity but zero durable artifacts across multiple windows despite an explicit tests-first/outputs brief is non-converging: kill it and redispatch with a tightened forcing-function brief.

CTO dispatches only managers. Managers dispatch implementers/fix/review/verify agents. Codex is read-only reviewer/architect/decider only; all code/tests/fixes are by claude. Any codex-written code is suspect and redone by claude. Trust but verify: every sub-agent report is INTENT, not fact — confirm "done"/test-counts/commits/deletion claims against `git show --stat`, `git log`, `grep`, and gate summaries before acting.

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

Every block/stage/phase review round = 2 codex + 1 claude, parallel, neutral, distinct lenses, read-only, no tests, harsh production bar, to 3/3 LAND or NIT-only carried forward. Designs/docs get the same full 3/3; later landing/rebase skips re-review only when byte/hash-identical. Any conflict resolution, mirror content delta, or non-identical "mechanical" change re-enters 3/3 + §1a + anti-rogue layer 1. Skill/design/doc codex reviews cap at 3 rounds; after 3, finalize cosmetic/wording/framing residuals, but substantive or anti-rogue findings still block.

Findings route by scope. In-scope P0/P1/P2 enter normal fix cycle; P0 blocks, each fix is a new commit, never amend, re-review until clean. Scope-deviating/increasing findings (system-wide/pre-existing class, architecture beyond block, relitigating settled scope) go to codex with plan context for adopt-now/defer/reject. Fix whole classes within the block surface; if class extends beyond deliverable, stop and codex-scope-consult. If recurring out-of-scope findings exceed about 5 rounds, consult early. Adjudicate reviewer compile/test claims against the verified gate: if the full gate compiled and ran, a false "won't compile" claim is INVALID — never add a no-op patch merely to clear that verdict.

## Architecture / Decision Modes

Architecture is always codex-owned. Claude executes; codex never writes code. Iterate until confident; never accept hedged verdicts. If codex contradicts verified repo evidence after rerun, stop/escalate.

Single yes/no architecture question → one neutral codex leg. Genuine multiple-choice/high-stakes fork → two neutral codex legs in parallel: A options-framed, B problem-only/unprimed. If they agree, decision is binding. If they disagree, run a third codex decider with both outputs plus source; it verifies decisive claims against code and adjudicates.

Modes:
- `escalate-to-user`: codex produces architecture analysis; user ratifies/rejects. Product/priority forks go to user.
- `full-autonomous`: auto-adopt codex architecture verdict; derive product/priority from approved plan.
User never adjudicates architecture instead of codex. Stop/user boundaries: no implementation account, destructive operation (force-push/history rewrite/irreversible shared state), unrecoverable lost/corrupt worktree with no trusted commit, codex unavailable/contradicts evidence, or product/priority choice not derivable from plan.

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

After every manager land, CTO dispatches a separate unprimed confirm MANAGER. It independently verifies four bars and writes `VERDICT:CONFIRMED` only when all hold:
1. CORRECT + additive/non-breaking-as-claimed + full gate GREEN first-hand; re-grep legacy deletion; re-prove every correctness-bearing test/guard discriminates in throwaway worktree (plant violation → RED → restore → GREEN); sampling one is invalid; `cmp` design mirrors.
2. NOT SHALLOW: real inputs and edges; no non-discriminating characterization.
3. NO STUBS: no empty/always-true tests or unconditional default/unknown/OK returns as implementation.
4. BEST IMPLEMENTATION: unprimed codex-architect judges whether a better design exists. Merely correct but not best = REOPEN.

On REOPEN, CTO dispatches fresh fix manager → re-land → re-confirm. Per-stage confirm is separate from integration-confirm.

Integration-confirm MANAGER runs at every phase/milestone boundary, before any dependent phase, before final close-out, and after every 5 confirmed blocks. It derives done-bar from binding plan, not reports; reviews integrated diff/tree; runs canonical gate; checks cross-block invariants, manifests, design mirrors, debt honesty, legacy deletion, hollow fronts, cache/perf/warm-state. Issues are classified via codex as REOPEN vs FEED-FORWARD. FEED-FORWARD only mid-plan. Only `VERDICT:INTEGRATION-CONFIRMED` closes phase.

Stage/phase cleanup only after land + confirmation: `git worktree remove` + `git worktree prune`, remove transient briefs/jsonl/review outputs/markers, preserve CTO ledger/MOM-NOTES, landed reports, debt ledger, design docs, verify clean status. At phase boundary clear all closed-stage worktrees/temp. Never delete live `/tmp` evidence mid-flight.

## Status Reporting

Write/overwrite `/tmp/mom/<BLOCK>/status.json` at milestones:

```json
{ "block":"<id>", "state":"RUNNING|BLOCKED|DONE|PARTIAL",
  "phase":"<short phrase>", "branch":"<branch>", "head":"<sha>",
  "commits":["<sha> <subj>"], "gate":{"rust":"PASS|FAIL|PENDING","js":"PASS|FAIL|NA"},
  "review":{"claude":"LAND|CHANGES|PENDING","codexA":"...","codexB":"..."},
  "newFailures":[], "note":"<=1 line" }
```

`BLOCKED`: first run escalation protocol; if unresolved, write `ESCALATION.md` with question, two options, what tried, set state, exit. `PARTIAL`: keep only internally clean committed branch work, no ff-merge unless full lifecycle completed; write `HANDOFF.md`; no final WIP commits. `DONE`: only after 3/3 LAND, §1a `VERDICT:LAND`, anti-rogue layer 1, rebase, post-rebase full gate, teeth'd squash, true ff, cleanup, `MANAGER-LANDED.md`, CTO ledger append, then exit.

## Verter Invariants

Never weaken: one resolver (`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`); typed-IR-only (no source-slicing/regex-on-type-text/reparse in resolver); shallow-by-default; fact-cache R21 five split env hashes; R6 no versioned identity in query-identity keys; §10.4.1 363-row manifest partition EXACT + bijective with live manifest; `AdditionalProofRow` closed at 7; `CodeTransform` is the only output-mutation path; final-state prose/no phase archaeology in `crates/*/src/**`.

Cross-platform: codebase must build/test/materialize on macOS/Windows/Linux. No NTFS-illegal chars (`< > : " | ? * \`, control chars, trailing dot/space, reserved CON/PRN/AUX/NUL/COM#/LPT#) in tracked paths or generated names; sanitize names. No hardcoded path separators; use `Path`/`PathBuf`/`join`. Byte-equality compares normalize CRLF/LF or compare text. OS binaries (`tsgo`/`.exe`) are discovered platform-aware. Temp/cwd use std abstractions. Platform-assuming code is a defect.

## Terseness

Rules/skills stay concise. New process gaps go to the ledger and, if rule-bearing, through GOVERNANCE before adoption.
