# CHECKPOINT / RESUME PROTOCOL

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

Every long-running manager/implementer/reviewer/confirmer may be restarted cold by transient failures. Recovery must continue from durable truth, not memory.

## Relaunch First

On every launch/relaunch:
1. Read `PROGRESS.md` if present.
2. Read durable state: `git log <base>..HEAD`, `git status --short`, and for managers review/verdict dirs plus report files (on `mechanism=claude-p`, also terminal files).
3. Resume at the next INCOMPLETE step.

Existence is not validity. Reuse an artifact only if its recorded input identity matches current inputs exactly: base/head SHA or diff hash, command, env/baseline, train/slice/round, reviewed path set as applicable. If identity is missing or changed, artifact is stale and reruns. Never let a stale review/gate/check satisfy a gate.

## PROGRESS.md

Append-only, newest at bottom, one terse factual line per event, no timestamps:
- `TRAIN <train-id> ...` / `SLICE <slice-id> ...`
- `COMMIT <sha> — <what>`
- `DISPATCH <role> mechanism=agent|claude-p brief=<path> report=<path> input-id=<hash> model=<id> effort=<level>` — EVERY gate-bearing dispatch records these five regardless of mechanism (gate-bearing roles persist brief+report to files, never inline). Mechanism-specific evidence is EXTRA only and never substitutes for the input identity: `agent-id=<id>` on the Agent path; `terminal=<path> wrapper-pid=<n>` + jsonl on the opt-in `claude -p` path.
- `CAPABILITY mechanism=agent isolation=<pass|fail> identity=<pass|fail> stopcontinue=<pass|fail> childspawn=<pass|fail> result=PASS|FAIL` — recorded once before the first Agent dispatch; absent or stale (inputs changed) ⇒ "unknown" ⇒ forces the `claude -p` fallback.
- `VERDICT <reviewer> lens=<A|B|C> = LAND|CHANGES base..head=<sha..sha> file=<path>` — one line per reviewer; a train review round records all three independent reviewers (author-dependent mix).
- `DECISION <what> — <why/codex-ref>`
- `CHECK <n> = PASS|FAIL inputs=<base..head|cmd|env> <evidence>`
- `CHECKPOINT manifest=<content-id> trains=<total>/<confirmed>/<active>/<remaining> rows=<open>/<closed> scope-adds=<n> impl-vs-wait=<t>/<t> reviews=<rounds> initial-p0p1=<n> reopens=<n> next=<exact finish condition>` — every CTO checkpoint records these fields; the denominator comes from the frozen manifest and cannot grow silently.
- `BLOCKED: <what>`

Every review/gate/verifier/codex/sub-agent entry records input identity. Entries without it are stale.

## Durable Artifacts

Implementer/fix commits per logical piece once it compiles, after cheap check (for example `cargo check -p <crate>`), not full gate. Targeted gates run during iteration; the canonical full pair runs at final acceptance on the rebased, landing-frozen tree and again independently at confirm. Every gate-bearing dispatch (any mechanism — Agent or `claude -p`; review, §1a, confirm, anti-rogue) persists the exact prompt/brief AND the verbatim final report to files before the report is acted on, recording an input identity/hash plus the resolved `model`+`effort` in `PROGRESS.md`; inline-only briefs are forbidden for those roles — anti-rogue and never-prime both require post-hoc inspection of the exact prompt and output. Reviewer/architect output always goes to files. Confirmer writes report incrementally with `VERDICT: PENDING`, then flips to CONFIRMED/ISSUES-FOUND. Manager appends train/slice results to `PROGRESS.md` and CTO ledger.

## Idempotence

Before skipping, prove "already done AND still valid." Before redispatching a worker, confirm none is live — Agent status/stop result on the default path, `ps` count 0 on the opt-in `claude -p` path. Do not double-apply edits, double-dispatch workers, or accept stale artifacts.

## Collision / Corruption

If a half-killed worker or race may have produced a commit, never continue from it:
1. Stop the worker: on the default Agent path, stop the background Agent (e.g. TaskStop) and confirm it is no longer running; on the opt-in `claude -p` path, kill both the resume-loop wrapper and the inner `claude -p` (`pkill -f BRIEF` alone misses the inner). Confirm zero workers.
2. Preserve all evidence: outputs, verdict dirs, `PROGRESS.md`, gate logs.
3. Reset/recreate from the last trusted commit recorded in `PROGRESS.md`, then resume.
4. If no trusted commit exists, STOP and escalate to CTO/user.

Agents/verifiers must not delete orchestration scratch or evidence directories. They are recovery substrate.
