---
ruling_id: "LANDING-IS-ORCHESTRATOR-ONLY"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["program-wide landing/orchestration protocol"]
source_file: "MAINTAINER-RULING-LANDING-IS-ORCHESTRATOR-ONLY.md"
summary: "A block/train orchestrator never writes to program/architecture-lock (no fast-forward, no merge, no commit) — it stops and reports once its own checks pass. The program orchestrator runs its own independent checks against that branch, performs the squash, and lands; landing authority is not delegated. Issues found are fixed directly or by spawning another orchestrator, never self-certified by the original block orchestrator. The gate is one of the program orchestrator's checks and may run in parallel with the others (still one gate at a time on the machine). Includes standing brief text to paste into every dispatched block/train brief."
supersedes: []
superseded_by: []
contradicts: []
notes: "Cites two same-day incidents as motivation (BV1 fast-forwarded itself into trunk by its own manager) versus BS1's manager doing the right thing unprompted (wrote its proposed transition and stopped) — the latter becomes the rule."
---

# Maintainer ruling — block orchestrators never land; the program orchestrator lands

**Date:** 2026-08-20.

> the block orchestrator should never land in the working branch, once it passes all the check it should
> stop and provide the report to you, you run the check in their branch and do squash and land by
> yourself, or spawn another orchestrator to fix any issues found, one of your checks is the gate, you
> can run gate in parallel with the other checks

## The protocol, now binding

1. **A block/train orchestrator NEVER writes to `program/architecture-lock`.** No fast-forward, no merge,
   no commit. It works on its own branch in its own worktree and stops.
2. When its own checks pass, it **STOPS and reports** — branch name, candidate SHA, what it did, what it
   verified, and what it could not.
3. **The program orchestrator runs its OWN checks against that branch**, independently. Trust-but-verify
   is not satisfied by reading the block's report.
4. **The program orchestrator performs the squash and the landing.** Landing authority is not delegated.
5. Issues found ⇒ either fix directly or **spawn another orchestrator** to fix them. The original block
   orchestrator does not get to self-certify a fix round into the trunk.
6. **The gate is one of the program orchestrator's checks**, and it MAY RUN IN PARALLEL with the other
   checks (review seats, identity verification, diff scans). Still ONE gate at a time on this machine.

## Why

Two blocks today were landed by their own managers (BV1 fast-forwarded itself into the trunk). That
places the verdict and the landing in the same hand — the block certifies itself and then writes the
result. Every false green found today came from a self-certifying step somewhere in the chain.

BS1's manager did the right thing unprompted: it wrote its proposed transition and stopped, explicitly
saying landing was not its call. That is now the rule, not the exception.

## Standing brief text (paste into every dispatched block/train brief)

> **You never land.** Do not fast-forward, merge, or commit to `program/architecture-lock`. Work only on
> your own branch in your own worktree. When your checks pass, write your report and STOP — the program
> orchestrator runs independent checks, performs the squash, and lands. If you find yourself typing
> `git merge`, `git rebase --onto` against the trunk, or `git push`, stop: that is not your job.

## Enforcement

A watcher on the `program/architecture-lock` tip detects any landing the program orchestrator did not
perform. There is no local git mechanism to make it impossible, so this is detection plus discipline.
