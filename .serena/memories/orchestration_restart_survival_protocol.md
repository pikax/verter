# Restart-Survival Protocol for Orchestrated Sub-Agents (MANDATORY)

## Problem (root cause)
This Claude Code environment restarts the session ~hourly. A restart KILLS all in-session
Agent-tool sub-agents and background Bash tasks. Implementer agents kept dying mid-work and
losing UNCOMMITTED WIP because the LONG pole — `cargo` verification (full `--tests` suite +
multi-build revert-and-observe discrimination proofs = 10-20 min) — ran INSIDE the agent and the
agent committed only AFTER verification. The restart almost always lands in that verification
window → the agent dies before committing → WIP lost (must be recovered by hand).

## Fix — the long pole must NEVER live inside a killable, un-banked agent
1. **Orchestrator applies SMALL codex-specified fixes DIRECTLY (no agent).** The orchestrator
   survives restarts (resumes via its handoff doc + a start-of-turn background-state audit). A
   1-line / few-line fix that codex has precisely specified → `Edit` on the ABSOLUTE worktree path
   → verify → commit, yourself. (Proven: recovered a dead agent's raise.rs `with_mode(Navigate)`
   one-liner this way in minutes.)
2. **Agents (for larger work) are COMMIT-FIRST, VERIFY-SECOND.** Every implementer brief MUST
   mandate: edit → `cargo check -p <crate>` (compiles only — fast) → `git commit` IMMEDIATELY →
   THEN run tests; on failure, follow-up fix commit. FORBIDDEN: running the full `--tests` suite or
   a multi-build revert-and-observe BEFORE the first commit. This caps agent wall-clock to a few
   minutes (fits inside a restart window) and guarantees a restart leaves a COMMITTED state.
3. **Orchestrator OWNS all heavy verification** (full `--workspace` gate, targeted test runs,
   discrimination revert-and-observe) on COMMITTED code — re-runnable after any restart (nothing
   lost). codex re-confirm judges test discrimination STATICALLY (reliable); the orchestrator does
   empirical revert-and-observe only if codex doubts.
4. **Keep the existing cheap-recovery rails:** WIP patch-backup (`git -C <wt> diff HEAD >
   <scratch>/x.patch`); AUDIT background state at the START of every turn (git log for new commits +
   newest mtime + `tasklist | grep cargo/rustc` + agent transcript mtime/size + the session-id dir
   to detect a restart); worktree-ABSOLUTE-path mandate (Serena + relative paths silently hit the
   MAIN repo); commit-after-every-step.

## Detecting a dead agent vs a live one (start-of-turn audit)
- New session-id directory under `…/Temp/claude/<project>/<session-id>/` = a restart happened →
  agents from the prior session id are DEAD.
- Agent transcript (`…/<session-id>/tasks/<agentId>.output`) stale mtime (minutes old) + 0/static
  size + no active `cargo|rustc` + uncommitted WIP = dead; recover. If cargo/rustc ACTIVE, it may
  be the agent's (or an orphaned) build still running — do NOT race it with competing cargo
  (concurrent cargo in one worktree can corrupt the target dir → transient `lld-link` crash).
- A `// BUG-REVERT-PROOF`-style marker in WIP = the agent died MID discrimination-proof (it
  reverted the fix to show the test fails, died before restoring) → restore the real fix.
