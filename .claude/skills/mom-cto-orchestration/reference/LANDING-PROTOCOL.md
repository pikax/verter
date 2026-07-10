# LANDING PROTOCOL — Train Landing + True FF

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

Landing executes as a CTO-scheduled, SHA-bound job. The CTO itself never runs the landing steps; the implementation manager never lands its own train; and neither the manager nor the landing job spawns the confirmer — only the CTO dispatches the independent confirm MANAGER after land, keeping the post-land gate independent of the author.

## Preconditions

Do not start landing until all hold:
- The full cumulative train diff has a FINAL clean 3/3 LAND from three independent blind reviewers using the author-dependent cross-model mix (Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT), or only NIT residuals carried forward. All three identities, models, lenses, immutable input SHA, and verdicts are recorded.
- §1a `VERDICT:LAND`: diff-scope, EVERY new/changed correctness-bearing test, guard, or refusal proven discriminating via its reversible mutation recipe (mutation → RED → restore → GREEN, plus the unplanted control) — sampling is forbidden; ratified-contract, CRITICAL-invariant, executable-obligation, and anti-rogue/rule integrity (plans supply sequencing/traceability only and never override those bars). §1a is targeted; the canonical full pair is NOT part of §1a — it is the final-acceptance gate in step 1 below.
- Anti-rogue layer 1 complete per `PROTOCOL.md`: full discovery over all changed paths, deleted hunks, deletions/renames/copies, and clean unprimed adversarial codex confirmation for each rule-bearing change.
- The landing LEASE is held: single-writer land window, one heavy suite per host, concurrent model reviews only on isolated workers.
- No in-flight worker for this train — Agent status/stop result confirms none live on the default path, `ps` count 0 on the opt-in `claude -p` path; no uncommitted junk.

## 1. Pre-Land Sync

Fetch and re-read current integration tip. Rebase the train onto current `refactor/semantic-db-overhaul`, then run the canonical FULL Rust pair on the rebased, landing-frozen tree — this is FINAL ACCEPTANCE; a cheap check is not sufficient, and targeted-gate successes are never landing evidence. Any later content change, conflict resolution, or regeneration invalidates this gate and re-runs it.

Re-review trigger: a clean zero-delta replay may proceed. Any conflict resolution or content/mirror delta re-enters full 3/3 + §1a + anti-rogue layer 1 before commit preparation. "Mechanical" does not bypass review.

## 2. Design Mirror

If the train authored/edited a binding design, ensure `docs/arch/<name>-design.md` is tracked, referenced by the master-plan locked-designs index, and byte-identical to the reviewed working design (`diff`/`cmp` empty). Stale or changed mirror content blocks land and may trigger re-review.

## 3. Train Commit Preparation

A landed train = one clean conventional commit (`<type>(<scope>): <desc>`) per reviewed slice, in reviewed order, plus one consolidated fix commit per review round — never WIP history. A single-slice train is still exactly one commit.

First run `git status --short`. Remove only verified untracked/ignored scratch created by this train; never remove by filename class. A tracked file is never cleanup-deleted. If an in-repo worktree or unowned untracked directory appears, STOP unless verified as this train's scratch with no live owner. Do not add per-file `.gitignore` entries.

When consolidating fix history or rebuilding a commit, use explicit scoped staging:
1. `git reset --soft <base-of-the-commit-being-rebuilt>`
2. `git reset` to unstage all.
3. `git add <explicit intended product paths>` only; never `git add -A` or `git add .`.
4. Verify `git diff --cached --name-only` equals the reviewed path set: no extras, no missing paths.
5. Commit with the conventional message.

Verify every commit in the landing range is intentional and the cumulative head tree is byte-identical to the finally reviewed/gated content. No `wip`/`fixup`/`squashme`/amend-only history; no attribution trailer. Scrub all plan/phase vocabulary from staged diffs and commit messages. The final canonical gate on the rebased, landing-frozen tree covers exactly this byte-identical range.

## 4. True Fast-Forward

Advance integration by true ff from the train head. No merge commit. Verify `git log --merges <old>..<new>` is empty and `git rev-list <old>..<new>` is exactly the ordered, reviewed slice + consolidated-fix commit range — every commit intentional; the range count is NOT required to equal one (a single-slice train is still exactly one commit). Never push. Never commit directly on the integration branch.

## 5. Cleanup + Report

Remove worktree and prune:
- `git worktree remove <worktree>`
- `git worktree prune`

Do NOT remove transient scratch yet — the CTO confirm + anti-rogue layer 2 still need it. At land remove only the worktree/build dirs (above); PRESERVE briefs, reports/logs, review outputs, gate logs, `PROGRESS.md`, design `cmp`/`diff` evidence (and on the opt-in `claude -p` path, `jsonl` stream logs + markers), and the durable record (CTO ledger/MOM-NOTES, landed report, debt ledger, design docs). Transient scratch is removed only AFTER land + CONFIRMED, per `PROTOCOL.md` train cleanup.

Write `MANAGER-LANDED.md` with land SHA(s), summary, three review verdicts, §1a verdict, gate pass line, load-bearing proofs (legacy deletion grep, design `cmp`/`diff`). Touch done marker and append CTO ledger. Verify repo status clean.

## CTO Handoff

CTO dispatches an independent unprimed confirm MANAGER. It reruns the four bars in `PROTOCOL.md` — including a fresh canonical full gate and independent re-execution of EVERY §1a mutation recipe — and anti-rogue layer 2 on the explicit landed range (`<old-integration-tip>..<new-integration-tip>`): discovery over all statuses, new bodies, deleted hunks, old bodies for deletions/renames, copy source; codex-confirm each rule-bearing change. Only `VERDICT:CONFIRMED` advances. REOPEN → fresh fix manager → re-land → re-confirm.

Green gate is necessary, not sufficient. Hollow/partial/last-wins implementations and non-discriminating tests block landing.
