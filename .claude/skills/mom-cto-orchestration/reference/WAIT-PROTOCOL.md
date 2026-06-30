# WAIT PROTOCOL — Foreground, Non-Polling

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

Applies to EVERY non-orchestrator role (block manager, implementer, fix, reviewer, verifier, confirmer), every mechanism. It removes two failures: (a) BACKGROUND-THEN-YIELD — ending the turn to be notified later strands a sub-agent the harness never re-wakes; (b) POLLING — a multi-call sleep/check loop burns tokens processing ongoing output. Only the top-level durable orchestrator session — the one the harness reliably re-wakes (the CTO under MoM/CTO) — may dispatch true background-and-be-notified work.

## The only correct wait

A non-orchestrator role waits FOREGROUND with a SINGLE blocking invocation that returns ONLY on a terminal state — clean finish, error, or timeout. Inside that ONE foreground invocation, shell backgrounding (`&` + `wait`) is fine — that is HOW the call blocks on the job, NOT a background-then-yield (the turn never ends):

```bash
# wait on one job; output only when it ends (portable — no GNU `timeout` needed):
<cmd> > OUT.txt 2>&1 & PID=$!; wait "$PID"; echo "DONE rc=$?"
# bound it with the harness/tool-call timeout (preferred), or a portable self-timeout:
( <cmd> > OUT.txt 2>&1 ) & PID=$!; ( sleep <generous-seconds>; kill "$PID" 2>/dev/null ) & T=$!; wait "$PID"; rc=$?; kill "$T" 2>/dev/null; echo "rc=$rc"
```

GNU `timeout` is NOT on every platform (e.g. macOS lacks it) — prefer the harness/tool-call timeout, or `gtimeout` / the self-timeout above. The invocation blocks IN-TURN until the work finishes / errors / times out; it does NOT stream or print ongoing output, and the turn never ends waiting. On return, read the small terminal artifact (`OUT.txt` / the codex `-o` file / a marker), then continue. Pick the timeout generously enough that normal completion returns before it, and treat a timeout as a real signal (a hang or an under-budget run), not a routine step. Never loop multiple tool calls polling a marker; never background a task and yield.

## Concurrent review legs

The one allowed non-orchestrator parallelism: run the review legs CONCURRENTLY, then block FOREGROUND for all in ONE wait. Mechanics differ by leg type, but both stay foreground:
- codex CLI legs: start each as a background process writing its own `OUT`/`-o` file INSIDE one foreground bash invocation, then `wait` on all their PIDs in that same invocation (it returns only when all are done).
- harness Agent legs: dispatch them as blocking calls and block in-turn for their reports. (The top-level orchestrator MAY instead dispatch Agent legs as notified-on-completion work — its prerogative as the durable session — but a child manager blocks foreground.)
No poll loop, no yield. Do not run a heavy / canonical gate concurrently with codex (memory contention) — serialize those.

## Recovery

A real socket / API death can still strand a turn; `CHECKPOINT-PROTOCOL.md` relaunch-first recovers from durable truth (`PROGRESS.md` + git), not memory. Do not confuse a rare death with the yield-to-wait or poll-loop traps this protocol removes.
