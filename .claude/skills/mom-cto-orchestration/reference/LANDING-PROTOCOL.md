# LANDING PROTOCOL — Teeth'd Squash + True FF

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

The block MANAGER lands; the CTO never lands and dispatches the independent confirm manager afterward — the block manager never spawns its own confirmer, so the post-land gate stays independent of the author.

## Preconditions

Do not start landing until all hold:
- Full block diff has 3/3 LAND: 2 codex + 1 claude, or only NIT residuals carried forward.
- §1a `VERDICT:LAND`: diff-scope, every new/changed correctness-bearing test/guard proven discriminating (plant/revert RED→GREEN) plus unplanted control, cold full gate, rule/plan integrity.
- Anti-rogue layer 1 complete per `PROTOCOL.md`: full discovery over all changed paths, deleted hunks, deletions/renames/copies, and clean unprimed adversarial codex confirmation for each rule-bearing change.
- No in-flight worker for this block — Agent status/stop result confirms none live on the default path, `ps` count 0 on the opt-in `claude -p` path; no uncommitted junk.

## 1. Pre-Land Sync

Fetch and re-read current integration tip. Rebase branch onto current `refactor/semantic-db-overhaul`, then rerun the FULL canonical gate. A cheap check is not sufficient.

Re-review trigger: a clean zero-delta replay may proceed. Any conflict resolution or content/mirror delta re-enters full 3/3 + §1a + anti-rogue layer 1 before squash. "Mechanical" does not bypass review.

## 2. Design Mirror

If the block authored/edited a binding design, ensure `docs/arch/<name>-design.md` is tracked, referenced by the master-plan locked-designs index, and byte-identical to the reviewed working design (`diff`/`cmp` empty). Stale or changed mirror content blocks land and may trigger re-review.

## 3. Teeth'd Squash

Landed block = one squashed conventional commit (`<type>(<scope>): <desc>`), not WIP history.

First run `git status --short`. Remove only verified untracked/ignored scratch created by this block; never remove by filename class. A tracked file is never cleanup-deleted. If an in-repo worktree or unowned untracked directory appears, STOP unless verified as this block's scratch with no live owner. Do not add per-file `.gitignore` entries.

Squash with explicit scoped staging:
1. `git reset --soft <merge-base-with-integration>`
2. `git reset` to unstage all.
3. `git add <explicit intended product paths>` only; never `git add -A` or `git add .`.
4. Verify `git diff --cached --name-only` equals the reviewed path set: no extras, no missing paths.
5. Commit with conventional message.

Tree must be byte-identical to reviewed/gated content. No `wip`/`fixup`/`squashme`; no attribution trailer. Scrub all plan/phase vocabulary from staged diff and commit message. The post-rebase full gate is the full gate for this byte-identical conventional commit.

## 4. True Fast-Forward

Advance integration by true ff from the squashed branch commit. No merge commit. Verify `git log --merges <old>..<new>` is empty and `git rev-list --count <old>..<new>` is `1`. Never push. Never commit directly on the integration branch.

## 5. Cleanup + Report

Remove worktree and prune:
- `git worktree remove <worktree>`
- `git worktree prune`

Do NOT remove transient scratch yet — the CTO confirm + anti-rogue layer 2 still need it. At land remove only the worktree/build dirs (above); PRESERVE briefs, reports/logs, review outputs, gate logs, `PROGRESS.md`, design `cmp`/`diff` evidence (and on the opt-in `claude -p` path, `jsonl` stream logs + markers), and the durable record (CTO ledger/MOM-NOTES, landed report, debt ledger, design docs). Transient scratch is removed only AFTER land + CONFIRMED, per `PROTOCOL.md` stage/phase cleanup.

Write `MANAGER-LANDED.md` with land SHA, summary, three review verdicts, §1a verdict, gate pass line, load-bearing proofs (legacy deletion grep, design `cmp`/`diff`). Touch done marker and append CTO ledger. Verify repo status clean.

## CTO Handoff

CTO dispatches an independent unprimed confirm MANAGER. It reruns the four bars in `PROTOCOL.md` and anti-rogue layer 2 on the explicit landed range (`<old-integration-tip>..<new-integration-tip>` or `<commit>^..<commit>`): discovery over all statuses, new bodies, deleted hunks, old bodies for deletions/renames, copy source; codex-confirm each rule-bearing change. Only `VERDICT:CONFIRMED` advances. REOPEN → fresh fix manager → re-land → re-confirm.

Green gate is necessary, not sufficient. Hollow/partial/last-wins implementations and non-discriminating tests block landing.
