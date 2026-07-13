# Restart survival — why the long pole must not live inside a killable agent

> **This memory records INSIGHT, never a command.** Operational prescriptions — dispatch,
> waiting, commit cadence, verification ownership, cleanup, termination — have exactly ONE
> current authority: `.claude/skills/mom-cto-orchestration/reference/`
> (`PROTOCOL.md`, `WAIT-PROTOCOL.md`, `CHECKPOINT-PROTOCOL.md`). This file is not a protocol
> and does not override one; it explains a failure mode so the protocol's rules read as
> reasoned rather than arbitrary. Read the protocol; do not run this file.

## The failure mode (root cause)

This environment restarts the session periodically. A restart kills in-session Agent sub-agents
and background tasks. Implementer agents kept dying mid-work and losing UNCOMMITTED WIP because
the LONG POLE — full `cargo` verification, plus multi-build revert-and-observe discrimination
proofs — ran INSIDE the agent, and the agent committed only AFTER verification. A restart almost
always lands in that verification window, so the agent dies before committing and the work is
lost.

The generalisable insight: **any completion-critical work that is not yet banked to a durable
artifact is at the mercy of whatever kills its process.** The longer the unbanked window, the
more likely something lands in it.

## What follows from it

- **Bank early, verify after.** An implementer commits per logical piece once it compiles (a
  cheap check, not the full gate); the full gate runs at the end. This caps the unbanked window
  to minutes and guarantees a restart leaves a COMMITTED state. The rule and its cadence live in
  `CHECKPOINT-PROTOCOL.md` → Durable Artifacts.
- **Heavy verification belongs where it is re-runnable.** A full-workspace gate run against
  COMMITTED code loses nothing to a restart — it just reruns. `PROTOCOL.md` → Verification Gate
  owns where gates run and how they are serialized.
- **Recovery reads durable truth, not memory.** On every relaunch, reconstruct state from the
  progress ledger and git, and resume at the next incomplete step; an artifact is reusable only
  if its recorded input identity still matches. `CHECKPOINT-PROTOCOL.md` → Relaunch First.
- **Detecting a dead worker is not the same as owning it.** Staleness signals (no new commits, a
  stale transcript, no active build process) tell you a worker is probably gone. They do NOT
  tell you which processes are yours. Termination is scoped to a RECORDED process tree, never to
  a name, pattern, or port match — that reaches sibling legs and the user's own sessions
  (`PROTOCOL.md` → Cleanup; `WAIT-PROTOCOL.md`).
- **Do not "solve" this by having the orchestrator write the code.** An earlier version of this
  note advised the orchestrator to apply small fixes directly because it survives restarts. That
  trades a recoverable problem for an unrecoverable one: the orchestrator's context fills with
  implementation detail and the independent-review gate loses the author/reviewer separation it
  exists to enforce. The orchestrator dispatches, decides, and verifies; it does not implement
  (`/multi-agent-orchestration` → The orchestrator role).
