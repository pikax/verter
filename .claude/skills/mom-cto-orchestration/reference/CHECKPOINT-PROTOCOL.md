# CHECKPOINT / RESUME PROTOCOL

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

Every long-running manager/implementer/reviewer/confirmer may be restarted cold by transient failures. Recovery must continue from durable truth, not memory.

## Relaunch First

On every launch/relaunch:
1. Read `PROGRESS.md` if present.
2. Read durable state: `git log <base>..HEAD`, `git status --short`, and for managers review/verdict dirs plus report files (on `mechanism=claude-p`, also terminal files).
3. Resume at the next INCOMPLETE step.

Existence is not validity. Reuse an artifact only if its recorded input identity matches current inputs exactly: base/head SHA or diff hash, command, env/baseline, phase/round, reviewed path set as applicable. If identity is missing or changed, artifact is stale and reruns. Never let a stale review/gate/check satisfy a gate.

## PROGRESS.md

Append-only, newest at bottom, one terse factual line per event, no timestamps:
- `PHASE ...`
- `COMMIT <sha> — <what>`
- `DISPATCH <role> mechanism=agent|claude-p brief=<path> report=<path> input-id=<hash> model=<id> effort=<level>` — EVERY gate-bearing dispatch records these five regardless of mechanism (gate-bearing roles persist brief+report to files, never inline). Mechanism-specific evidence is EXTRA only and never substitutes for the input identity: `agent-id=<id>` on the Agent path; `terminal=<path> wrapper-pid=<n>` + jsonl on the opt-in `claude -p` path.
- `CAPABILITY mechanism=agent isolation=<pass|fail> identity=<pass|fail> stopcontinue=<pass|fail> childspawn=<pass|fail> result=PASS|FAIL` — recorded once before the first Agent dispatch; absent or stale (inputs changed) ⇒ "unknown" ⇒ forces the `claude -p` fallback.
- `CODEX_PIN role=<role> policy=<authority-path>@<schema> model=<policy-model> effort=<policy-effort> available=<yes|no> cli=<bin>@<version> banner=MATCH|MISMATCH run=<run-id>` — one row PER ROLE (review, §1a, anti-rogue, architecture, best-implementation adjudication), recorded at preflight (step zero) before the first codex dispatch, persisting the COMPLETE mapping with the run identity so any leg's provenance is reconstructible afterwards. `model`/`effort` are POLICY, read from `CODEX_MODEL_POLICY[role]` in the ratified authority (`reference/codex-model-policy.toml`) — preflight records whether that policy-selected model is AVAILABLE and what actually BOUND; it never selects. Preflight discovers availability; it cannot supply intent. No ledger row, brief, or rule file substitutes a literal for the policy value. **Unavailable, substituted, unknown, or banner-mismatched ⇒ BLOCK the leg** — never substitute, upgrade, downgrade, or reuse another role's entry: a blocked leg is loud, a substituted reviewer is silent.
- `CODEX_DISPATCH id=<dispatch-id> run=<run-id> role=<role> os=<os> pid=<n> prompt=<path> out=<path> timeout=<s> attempt=<n> model=<policy-model> effort=<policy-effort> state=RUNNING|OK|TIMEOUT|FAILED` — written BEFORE blocking on the leg. A dispatch that never recorded what it spawned cannot prove which tree it owns, so "terminate only your own tree" would be unexecutable prose. Cleanup goes through the ONE shared helper — `terminate_recorded_tree` (`PROTOCOL.md` → Ownership and Termination) — which terminates the recorded tree and CONFIRMS the closure is gone. A bare PID kill does not reap descendants, and PIDs are reused: the record is the ownership claim, the confirmation is the proof, and neither is satisfied by a kill command that returned zero.
- `CODEX_RESULT id=<dispatch-id> exit=<code> banner=<observed-model>/<observed-effort> out=<path> verdict=LAND|CHANGES` — the verdict is READ from the leg's output file at `out`, never scanned for. Exit, banner, and a present non-empty `out` are the three preconditions; if any fails the leg DID NOT RUN and no `verdict` may be recorded. A `VERDICT` accepted from a keyword scan of the log is invalid — `LAND`/`CHANGES`/`__DONE__` all occur in the prompt echo.
- `VERDICT <reviewer> = LAND|CHANGES base..head=<sha..sha> file=<path>`
- `DECISION <what> — <why/codex-ref>`
- `CHECK <n> = PASS|FAIL inputs=<base..head|cmd|env> <evidence>`
- `BLOCKED: <what>`

Every review/gate/verifier/codex/sub-agent entry records input identity. Entries without it are stale.

## Durable Artifacts

Implementer/fix commits per logical piece once it compiles, after cheap check (for example `cargo check -p <crate>`), not full gate. Full gate runs once at the end. Every gate-bearing dispatch (any mechanism — Agent or `claude -p`; review, §1a, confirm, anti-rogue) persists the exact prompt/brief AND the verbatim final report to files before the report is acted on, recording an input identity/hash plus the resolved `model`+`effort` in `PROGRESS.md`; inline-only briefs are forbidden for those roles — anti-rogue and never-prime both require post-hoc inspection of the exact prompt and output. Reviewer/architect output always goes to files. Confirmer writes report incrementally with `VERDICT: PENDING`, then flips to CONFIRMED/ISSUES-FOUND. Manager appends phase results to `PROGRESS.md` and CTO ledger.

## Idempotence

Before skipping, prove "already done AND still valid." Before redispatching a worker, confirm none is live — Agent status/stop result on the default path; on the opt-in `claude -p` path, every RECORDED dispatch for this block is in a terminal state and its recorded tree confirmed gone. A global process count is NOT the check: `ps`/`tasklist` cannot distinguish this block's workers from another block's or the user's own, so a nonzero count blocks a legitimate redispatch and a zero count can be a lie about the tree you actually own. Do not double-apply edits, double-dispatch workers, or accept stale artifacts.

## Collision / Corruption

If a half-killed worker or race may have produced a commit, never continue from it:
1. Stop the worker: on the default Agent path, stop the background Agent (e.g. TaskStop) and confirm it is no longer running; on the opt-in `claude -p` path, terminate the recorded tree through `terminate_recorded_tree` (`PROTOCOL.md` → Ownership and Termination). Be precise about what the ledger holds: the recorded PID is the **wrapper** (`$!` is the `bash -c` that was spawned), and the inner `claude -p` is a DESCENDANT of it — it is reaped because the helper enumerates and confirms the descendant CLOSURE, not because the recorded PID is the CLI. Killing the recorded PID alone leaves the CLI running. Never terminate by pattern or image name (`pkill -f …`, `taskkill /F /IM`, `killall`) — that reaches sibling dispatches and the user's own processes. Confirm the closure is gone.
2. Preserve all evidence: outputs, verdict dirs, `PROGRESS.md`, gate logs.
3. Reset/recreate from the last trusted commit recorded in `PROGRESS.md`, then resume.
4. If no trusted commit exists, STOP and escalate to CTO/user.

Agents/verifiers must not delete orchestration scratch or evidence directories. They are recovery substrate.
