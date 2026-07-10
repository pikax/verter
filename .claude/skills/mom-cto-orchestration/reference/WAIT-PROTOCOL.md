# WAIT PROTOCOL

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

**Scope: the OPT-IN `claude -p` path only.** Default Agent/Task-tool dispatch is harness-managed — a blocking Agent call returns the sub-agent's final message; a `run_in_background` Agent call notifies the spawner on completion. No foreground-poll, no resume-loop, no marker watchdog. Everything below applies ONLY when you have explicitly opted into `claude -p` CLI subprocesses (separate-account / out-of-session work).

Headless `claude -p` agents are not interactive. When they stop tool calls and emit a final message, the process exits. They are not re-invoked by background notifications.

## Forbidden Waits

Never background a long task and yield. Never use ScheduleWakeup/Monitor/"I'll resume when done" in headless `-p`. That exits and the resume-loop restarts cold.

## Only Correct Wait

Wait in a foreground blocking poll-loop, chunked to ≤5 minutes per tool call:
1. Start long task detached, writing output and marker:
   `nohup bash -c '<cmd> > OUT.txt 2>&1; echo DONE > MARKER' >/dev/null 2>&1 &`
2. Poll foreground:
   `for i in $(seq 1 10); do [ -f MARKER ] && { echo READY; break; }; sleep 30; done; [ -f MARKER ] && echo READY || echo STILL-RUNNING`
3. If `STILL-RUNNING`, emit one short status line, then run the next ≤5-minute poll chunk.
4. On `READY`, read output and continue.

A single 15-minute blocking call can hit idle timeout; chunking plus brief model output keeps the turn alive.

## Gates / Concurrent Work

If a gate is expected under 5 minutes, run foreground with `timeout 300`. Otherwise detach + chunk-poll. Only the CTO-owned orchestration process waits for review or verifier jobs on this CLI path; implementation managers never background-dispatch a gate or reviewer and then wait or yield. Dispatch the concurrent jobs in background — each with a marker — then run ONE foreground watchdog over all markers. A canonical/full (heavy) gate overlaps the THREE reviewer jobs (author-dependent model mix: Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT) ONLY on isolated workers; on a single host, serialize the heavy gate against reviewer processes — one heavy suite per host. Targeted non-heavy jobs may run concurrently either way. When any marker is ready, collect it and re-enter the watchdog for remaining markers. Never background the watchdog and yield. Completion markers are notifications, not verdicts — act only on the durable summary/report artifact.

## Recovery

Real socket deaths can still happen; the resume-loop plus `CHECKPOINT-PROTOCOL.md` recovers. Do not confuse rare API death with the yield-to-wait trap.
