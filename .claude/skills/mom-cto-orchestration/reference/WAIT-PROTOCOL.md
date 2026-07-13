# WAIT PROTOCOL

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

## The Turn Rule — applies on EVERY path

**A turn ends only when (a) you are blocked on a synchronous tool result you just invoked, (b) your completion-critical work has a HARNESS-GUARANTEED RESUME that will wake you, or (c) your work is committed and your report is written.**

The invariant behind all three: **never yield while completion-critical owned work lacks a harness-guaranteed resume or an active synchronous join.** A raw subprocess (codex, a shell gate) has NEITHER — nothing will wake you when it finishes, so YOU must be the thing that waits.

That is a rule about the TURN, not about the process. Do not read it as "the workload runs in the foreground": a short subprocess (a codex leg) runs foreground under a bounded timeout and the blocking call IS the join; a LONG one (the heavy Verification Gate) is DETACHED with a marker and then chunk-polled from the foreground. In both cases the turn stays awake and joined to the work. What is forbidden is detaching and YIELDING — walking away from work nothing will wake you for. A harness Agent call HAS a guaranteed resume (a blocking call returns the report; a `run_in_background` call notifies on completion), which is why (b) exists and is not a loophole. The failure being closed is yielding on work that nothing will wake you for.

Scoping this rule to one dispatch path is how the stall class survives: the thing that backgrounds is precisely the thing that escapes a narrowly-scoped rule.

Per path:
- **codex** — run BLOCKING in the foreground with an explicit bounded timeout, transcript to a file and the verdict to its own file (`PROTOCOL.md` → codex Invocation). No trailing `&`, never detached, never background-and-polled. **On expiry `timeout` has ALREADY terminated the leg** — there is no recorded PID and no separate kill step, which is the point of foreground: FAIL the leg and REDISPATCH under the retry ceiling. Parallelism comes from separate managed review calls, never from global shell process manipulation.
- **Agent/Task tool (default)** — harness-managed: a blocking Agent call returns the sub-agent's final message; a `run_in_background` Agent call notifies the spawner on completion. No foreground-poll, no resume-loop, no marker watchdog needed.
- **`claude -p` (opt-in)** — the chunked foreground poll-loop below. Headless `-p` agents are not interactive: when they stop tool calls and emit a final message the process exits, and they are NOT re-invoked by background notifications.

## Forbidden Waits

Never background a long task and yield **with nothing that will wake you** — that is the whole failure, and it is why the Turn Rule above has three permitted states rather than two. A `run_in_background` Agent call is NOT this defect: the harness resumes you on completion. A detached shell task with no join and no resume IS. Never use ScheduleWakeup/Monitor/"I'll resume when done" in headless `-p`: that exits, and the resume-loop restarts cold.

## Only Correct Wait (opt-in `claude -p` path)

Wait in a foreground blocking poll-loop, chunked to ≤5 minutes per tool call.

1. Start the long task detached — and RECORD WHAT YOU SPAWNED in the same breath. A detach that does not capture `$!` cannot obey the ownership rule below; it leaves a tree nobody can name, so the only cleanup left is a pattern kill, which is forbidden.

   Steps 1–4 are ONE lifecycle, not four independent snippets: the launch fence deliberately does not report the child's failure — nothing can, at launch time — so the collector in step 4 is what makes the whole thing honest. Running 1 without 4 is a dispatch with no gate.

   Three rules are baked in, and skipping any one produces a confident false green:

   - **The marker carries the task's EXIT STATUS, never the word `DONE`.** A bare `; echo DONE > MARKER` writes the marker even when the task FAILED — the poll loop reports READY, and a failed job is collected as a finished one.
   - **Marker publication is ATOMIC.** `echo $? > "$MARKER"` creates the file BEFORE it writes to it, so a poller can observe an empty marker and collect a task whose status does not exist yet. Write to a temp file and `mv` it into place — a rename is atomic, so the marker either is not there or is complete.
   - **Paths are PER-DISPATCH.** Two concurrent tasks sharing a literal `OUT.txt`/`MARKER` let one task's completion satisfy the other's watchdog: the poller sees a marker, calls READY, and reads output belonging to something else.

   ```bash
   OUT="$RUN_DIR/$DISPATCH_ID.out"; MARKER="$RUN_DIR/$DISPATCH_ID.marker"
   # A marker left by a PREVIOUS attempt at this id is a stale success that satisfies today's
   # check instantly — and a REMOVAL THAT FAILS leaves it there, so the removal is checked too.
   rm -f "$OUT" "$MARKER" "$MARKER.tmp" \
     || { echo "cannot clear stale artifacts — refusing to dispatch"; exit 1; }

   # `setsid` makes the child a process-group LEADER, which is what terminate_recorded_tree signals
   # on POSIX. It is NOT present in git-bash on Windows — hardcoding it there fails the launch
   # outright ("setsid: command not found") and the child never starts — so it is applied only where
   # it exists, and the helper falls back to the enumerated descendant closure everywhere else.
   SETSID=""; command -v setsid >/dev/null 2>&1 && SETSID="setsid"   # literal we set; unquoted on purpose

   # Two things here are load-bearing, and each closes a false-green that reads exactly like success:
   #
   # `-o errexit -o pipefail` on the INNER shell — the one that runs $CMD. Without them the marker
   # records the status of the LAST command in $CMD, not of $CMD: `false; true` and
   # `cargo nextest … | tee log` both publish marker=0, and the collector then reports a genuine
   # success for a task that failed. The marker is only as honest as the shell that computes it.
   #
   # ARGS, not interpolation. $CMD/$OUT/$MARKER are positional args into a single-quoted body. With
   # `bash -c "$CMD > $OUT 2>&1; …"` the redirect binds to the LAST command inside $CMD, so a compound
   # $CMD sends its output nowhere and, if it ends in `exit`, dies before publishing the marker.
   $SETSID nohup bash -c 'bash -o errexit -o pipefail -c "$1" > "$2" 2>&1; echo $? > "$3.tmp"; mv "$3.tmp" "$3"' \
     _ "$CMD" "$OUT" "$MARKER" >/dev/null 2>&1 &
   WRAPPER_PID=$!   # capture before any other async launch overwrites $!

   # The LEDGER is the durable ownership record. If the append fails we hold a live child we
   # could never prove we own after a restart — the orphan this protocol forbids. We still have
   # the PID in-shell right now, so terminate it while we can rather than leak it.
   if ! echo "DISPATCH wrapper-pid=$WRAPPER_PID out=$OUT marker=$MARKER" \
          >> "$RUN_DIR/PROGRESS.md"; then
     echo "ledger append FAILED — terminating the child rather than orphaning it"
     if ! terminate_recorded_tree "$WRAPPER_PID"; then
       # BOTH failed: we can neither RECORD the child nor KILL it. A warn-and-exit here abandons a
       # live tree that nothing owns — the orphan this protocol forbids, dressed as a handled error.
       # The transcript becomes the record of last resort, so it must carry the PID, and the run
       # STOPS rather than stacking further dispatches on top of an untracked process.
       echo "BLOCK: unrecorded, unterminated child wrapper-pid=$WRAPPER_PID out=$OUT"
       echo "BLOCK: ledger unwritable AND termination unconfirmed — HUMAN CLEANUP REQUIRED"
       exit 2   # distinct from 1: not a failed dispatch — an UNOWNED LIVE TREE
     fi
     exit 1
   fi
   ```

2. Poll foreground (9 × 30s keeps the call under the 5-minute ceiling once overhead is counted). The poll watches for TWO outcomes, because a child can die without ever publishing a marker — if you only look for the marker, that death polls forever:

   Liveness comes from the shared `pid_alive` helper (`PROTOCOL.md` → Ownership and Termination), never from `kill -0`: that signal fails both for "no such process" AND for "permission denied", so it cannot tell a dead wrapper from a live one you may not signal — and PROTOCOL.md already rejects it as an oracle for exactly that reason. Two pages confirming the same fact must not use two oracles, one of which the other calls unsound. `pid_alive` returns 0 alive / 1 gone / **2 cannot tell** — and "cannot tell" is never "gone".

   ```bash
   for i in $(seq 1 9); do
     [ -f "$MARKER" ] && break
     pid_alive "$WRAPPER_PID" || break          # gone (or undecidable): stop polling, decide below
     sleep 30
   done
   pid_alive "$WRAPPER_PID"; ALIVE=$?
   if [ -f "$MARKER" ]; then echo READY
   elif [ "$ALIVE" -eq 0 ]; then echo STILL-RUNNING
   elif [ "$ALIVE" -eq 2 ]; then echo "cannot read the process table — treat as FAILED"; exit 1
   else echo "DIED WITHOUT A MARKER — treat as FAILED"; exit 1
   fi
   ```

3. If `STILL-RUNNING`, emit one short status line, then run the next poll chunk.
4. On `READY`, **collect the status before you read anything else.** A present marker means the task ENDED, not that it succeeded — and a marker that is present but unreadable is a FAILURE, never a pass:

   ```bash
   # Check the READ itself: an unreadable marker whose partial content happens to be "0" would
   # otherwise be collected as a success. The read failing is a failure, not a zero.
   TASK_EXIT="$(cat "$MARKER")" || { echo "cannot read $MARKER — treat as FAILED"; exit 1; }
   case "$TASK_EXIT" in
     0)            : ;;                                                   # genuinely succeeded
     ''|*[!0-9]*)  echo "MARKER CORRUPT/EMPTY — treat as FAILED"; exit 1 ;;
     *)            echo "TASK FAILED (exit=$TASK_EXIT) — see $OUT"; exit 1 ;;
   esac

   # The marker is published by `mv` BEFORE the wrapper exits, so a marker is NOT proof the wrapper is
   # gone — and this is not a `wait` (a detached wrapper is no longer our child, so we cannot join it).
   # Confirm the terminal state with the SHARED `pid_alive` helper, the same oracle the terminator uses.
   for i in $(seq 1 10); do
     pid_alive "$WRAPPER_PID" || break
     sleep 1
   done
   pid_alive "$WRAPPER_PID"; ALIVE=$?     # capture FIRST — testing $? after another command reads it
   if [ "$ALIVE" -eq 0 ]; then
     echo "WRAPPER $WRAPPER_PID still alive after publishing its marker — do NOT treat it as finished"
     exit 1
   elif [ "$ALIVE" -eq 2 ]; then
     echo "cannot read the process table — refusing to certify that wrapper $WRAPPER_PID exited"
     exit 1
   fi
   ```

A single 15-minute blocking call can hit idle timeout; chunking plus brief model output keeps the turn alive.

## Gates / Concurrent Work

If a gate is expected under 5 minutes, run it foreground under a bounded `timeout`. Otherwise detach + chunk-poll. codex legs are the exception: blocking-foreground with a bounded timeout, one per call (`PROTOCOL.md` → codex Invocation) — never detached with a marker; on expiry `timeout` has already terminated the leg, so FAIL it and redispatch under the retry ceiling. For other concurrent work (gate, claude reviewer, sub-agents) detach each with its OWN per-dispatch output and marker paths AND capture its `$!` before the wait (step 1 above), then run ONE foreground watchdog over all markers. When any marker is ready, collect its EXIT STATUS (not merely its existence) and re-enter the watchdog for the rest. Never background the watchdog and yield.

Terminate ONLY a process tree recorded as owned by the current dispatch, and do it through the ONE shared helper — `terminate_recorded_tree` (`PROTOCOL.md` → Ownership and Termination). Do not hand-roll the kill here: the obvious commands are wrong in ways that only show up when you run them (`$!` is not the pid `taskkill` accepts; `//T` does not reap descendants), so a second copy is both a drift surface and, historically, a broken one. A bare PID kill leaves every descendant running, and confirming the LEADER is gone while its children live is a false green — the helper enumerates the descendant closure from the recorded root, terminates it, and confirms the closure.

Never terminate by image name or pattern (`taskkill /F /IM`, `pkill -f …`, `killall`, `Stop-Process -Name`) — that reaches sibling legs, other dispatches, and the user's own processes, and a leg killed by someone else's cleanup looks exactly like a stalled leg.

## Recovery

Real socket deaths can still happen; the resume-loop plus `CHECKPOINT-PROTOCOL.md` recovers. Do not confuse rare API death with the yield-to-wait trap.
