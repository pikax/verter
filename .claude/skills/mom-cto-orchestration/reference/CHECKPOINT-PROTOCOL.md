# CHECKPOINT / RESUME PROTOCOL

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

Every long-running manager/implementer/reviewer/confirmer may be restarted cold by transient failures. Recovery must continue from durable truth, not memory.

## Relaunch First

On every launch/relaunch:
1. Read `PROGRESS.md` if present.
2. Read durable state: `git log <base>..HEAD`, `git status --short`, and for managers review/verdict dirs plus terminal files.
3. Resume at the next INCOMPLETE step.

Existence is not validity. Reuse an artifact only if its recorded input identity matches current inputs exactly: base/head SHA or diff hash, command, env/baseline, phase/round, reviewed path set as applicable. If identity is missing or changed, artifact is stale and reruns. Never let a stale review/gate/check satisfy a gate.

## PROGRESS.md

Append-only, newest at bottom, one terse factual line per event, no timestamps:
- `PHASE ...`
- `COMMIT <sha> — <what>`
- `DISPATCH <role> brief=<path> terminal=<path> wrapper-pid=<n>`
- `VERDICT <reviewer> = LAND|CHANGES base..head=<sha..sha> file=<path>`
- `DECISION <what> — <why/codex-ref>`
- `CHECK <n> = PASS|FAIL inputs=<base..head|cmd|env> <evidence>`
- `BLOCKED: <what>`

Every review/gate/verifier/codex/sub-agent entry records input identity. Entries without it are stale.

## Durable Artifacts

Implementer/fix commits per logical piece once it compiles, after cheap check (for example `cargo check -p <crate>`), not full gate. Full gate runs once at the end. Reviewer/architect output always goes to files. Confirmer writes report incrementally with `VERDICT: PENDING`, then flips to CONFIRMED/ISSUES-FOUND. Manager appends phase results to `PROGRESS.md` and CTO ledger.

## Idempotence

Before skipping, prove "already done AND still valid." Confirm `ps` count 0 before redispatching a worker. Do not double-apply edits, double-dispatch workers, or accept stale artifacts.

## Collision / Corruption

If a half-killed worker or race may have produced a commit, never continue from it:
1. Kill both resume-loop wrapper and inner `claude -p`; `pkill -f BRIEF` alone misses the inner process. Confirm zero workers.
2. Preserve all evidence: outputs, verdict dirs, `PROGRESS.md`, gate logs.
3. Reset/recreate from the last trusted commit recorded in `PROGRESS.md`, then resume.
4. If no trusted commit exists, STOP and escalate to CTO/user.

Agents/verifiers must not delete orchestration scratch or evidence directories. They are recovery substrate.
