# LANDING PROTOCOL — Teeth'd Squash + True FF

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

The block MANAGER lands; the CTO never lands and dispatches the independent confirm manager afterward — the block manager never spawns its own confirmer, so the post-land gate stays independent of the author.

## Preconditions

Do not start landing until all hold:
- Full block diff has 3/3 LAND: 2 codex + 1 claude, or only NIT residuals carried forward — and each carried NIT is enumerated in `PROGRESS.md` with an owner and a closure point (`SKILL.md`). An unenumerated residual is not "carried", it is dropped.
- §1a `VERDICT:LAND`: diff-scope, every new/changed correctness-bearing test/guard proven discriminating (plant/revert RED→GREEN, with the plant PROVEN PRESENT IN THE SOURCE — a mutation command's exit code is not proof it applied; see `PROTOCOL.md` → Plant Verification) plus unplanted control, cold full gate, rule/plan integrity.
- Anti-rogue layer 1 complete per `PROTOCOL.md`: full discovery over all changed paths, deleted hunks, deletions/renames/copies, and clean unprimed adversarial codex confirmation for each rule-bearing change.
- No in-flight worker for this block — Agent status/stop result confirms none live on the default path; on the opt-in `claude -p` path, every RECORDED dispatch for this block is in a terminal state, and for any dispatch this block TERMINATED, `terminate_recorded_tree` confirmed its closure gone. A global process count is not the check: it cannot distinguish this block's workers from another block's or the user's own.

  Say exactly what that does and does not establish, because the marker is weaker evidence than it looks. A detached wrapper is not our child, so nothing here `wait`s on it — and the marker is published by `mv` BEFORE the wrapper exits, so a present marker proves the TASK ended, not that the WRAPPER is gone. That is why the collector explicitly confirms the wrapper's terminal state instead of inferring it from the marker (`WAIT-PROTOCOL.md`). Even then: a dead wrapper does not prove it left nothing behind, and once it is gone its children are re-parented beyond the reach of the recorded root. So a normally-completed dispatch that leaked a descendant is NOT detected here, and this line does not claim otherwise. That is the same gap the terminator has, from the other direction, and it is one more reason enumerate-and-confirm is a stopgap: a containment object (GI-6) makes both cases a property of the child rather than a search of the process table.

## 1. Pre-Land Sync

Fetch and re-read current integration tip. Rebase onto the current integration branch named in the brief — never a branch name hardcoded in this protocol — then rerun the FULL canonical gate. A cheap check is not sufficient.

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

## 5. Report — and hand over in LANDED / AWAITING-CONFIRM

**The manager removes NOTHING here — not the scratch, and not the worktree.** Landing ends in an explicit `LANDED / AWAITING-CONFIRM` state: the commit is on the integration branch, the evidence is intact, and the manager exits.

This is the one sequencing that is consistent. The manager cannot both "clean up before exiting" and "not clean up until confirmation", because the CTO dispatches confirm only AFTER the manager has landed and exited — a manager that cleaned up at land would be destroying evidence for a gate that has not run yet. The reason already given for preserving the scratch (CTO confirm + anti-rogue layer 2 still need it) applies to the worktree for exactly the same reason, so both are preserved by the same rule.

PRESERVE: the worktree, briefs, reports/logs, review outputs, gate logs, `PROGRESS.md`, design `cmp`/`diff` evidence (and on the opt-in `claude -p` path, `jsonl` stream logs + markers), and the durable record (CTO ledger/MOM-NOTES, landed report, debt ledger, design docs).

`git worktree remove` + `git worktree prune` belong to STAGE/PHASE CLEANUP, owned by the CTO and run only AFTER land + CONFIRMED (`PROTOCOL.md` → Repo Cleanliness). Never remove a worktree whose owner may still be running, and never remove one before the gate that inspects it has passed.

Write `MANAGER-LANDED.md` (in the block's run dir, `$RUN_DIR/MANAGER-LANDED.md`) with land SHA, summary, three review verdicts, §1a verdict, gate pass line, load-bearing proofs (legacy deletion grep, design `cmp`/`diff`). Append the CTO ledger. Verify repo status clean.

`MANAGER-LANDED.md` IS the done marker — there is no second marker file. An earlier draft said "touch done marker" without naming a path, which is an instruction nobody can follow identically twice: two managers invent two names, and the CTO polls for a file that may never be written under the name it expects. A marker whose path is not specified is not a marker.

## CTO Handoff

CTO dispatches an independent unprimed confirm MANAGER. It reruns the four bars in `PROTOCOL.md` and anti-rogue layer 2 on the explicit landed range (`<old-integration-tip>..<new-integration-tip>` or `<commit>^..<commit>`): discovery over all statuses, new bodies, deleted hunks, old bodies for deletions/renames, copy source; codex-confirm each rule-bearing change. Only `VERDICT:CONFIRMED` advances. REOPEN → fresh fix manager → re-land → re-confirm.

Green gate is necessary, not sufficient. Hollow/partial/last-wins implementations and non-discriminating tests block landing.
