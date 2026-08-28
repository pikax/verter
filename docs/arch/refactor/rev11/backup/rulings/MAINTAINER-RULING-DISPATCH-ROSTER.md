---
ruling_id: "DISPATCH-ROSTER"
type: "maintainer-directive"
date: "2026-08-18"
date_source: "stated"
binds: ["program-wide dispatch discipline"]
source_file: "MAINTAINER-RULING-DISPATCH-ROSTER.md"
summary: "claude-max is the dispatch vehicle for implementers too, not only managers/orchestrators — supersedes an older 'use claude, not claude-max' note. A claude-max orchestrator may use Agent subagents for its own implementation fan-out. Unchanged: review seats stay external CLIs (codex/grok, never a Claude subagent); long-running workers launch in the foreground of a run_in_background:true Bash call, never a trailing &/nohup/setsid; a -p process is one-shot and the only sanctioned wait is a blocking foreground loop."
supersedes:
  - document: "an older, un-migrated 'use claude, not claude-max' dispatch note (not part of this corpus)"
    claim: "That implementers should be dispatched via claude, not claude-max."
superseded_by: []
contradicts: []
notes: ""
---

# Maintainer ruling — dispatch roster (2026-08-18)

> future claude-max should also be used for implementer, if claude-max is the orchestrator it's
> allowed to use subagents

1. **`claude-max` is the dispatch vehicle for IMPLEMENTERS too**, not only managers/orchestrators.
   Supersedes the older "use `claude`, not `claude-max`" note.
2. **A `claude-max` process acting as an orchestrator MAY use Agent subagents** for its own
   implementation fan-out. The earlier blanket "never use Claude subagents for implementation" applied
   to the program orchestrator's own habits, not to a claude-max manager's internal delegation.
3. **UNCHANGED — review seats stay EXTERNAL CLIs.** `codex` and `grok` only, never a Claude subagent as
   a review seat. That is a separate long-standing rule and this ruling does not touch it.
4. **UNCHANGED — launch mechanics.** Long-running workers go in the FOREGROUND of a
   `run_in_background: true` Bash call; never a trailing `&`, `nohup`, or `setsid`.
5. **UNCHANGED — a `-p` process is one-shot.** Ending its turn kills it. The only sanctioned wait is a
   blocking foreground loop.
