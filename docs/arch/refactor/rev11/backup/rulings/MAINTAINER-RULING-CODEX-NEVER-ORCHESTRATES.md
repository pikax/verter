---
ruling_id: "CODEX-NEVER-ORCHESTRATES"
type: "maintainer-directive"
date: "2026-08-18"
date_source: "stated"
binds: ["program-wide dispatch discipline"]
source_file: "MAINTAINER-RULING-CODEX-NEVER-ORCHESTRATES.md"
summary: "Codex is never an orchestrator — advisory only (review seat, architecture consult/ruling, scoping/premise verification), no write capability, never dispatches other agents, sequencing advice from codex remains advice the orchestrator decides on. Addendum, same day: codex is also not an implementer — supersedes an earlier 2026-08-05 reversal that had made codex the default implementer/fix agent; all implementation dispatches to claude-max."
supersedes:
  - document: "un-migrated 2026-08-05 note (not part of this corpus)"
    claim: "That codex is the default implementer/fix agent."
superseded_by:
  - ruling: "DISPATCH-ROSTER"
    claim: "This document's 'implementers are claude-max' framing is refined (not reversed) — claude-max becomes the dispatch vehicle for implementers too, and a claude-max orchestrator may use Agent subagents for its own fan-out."
contradicts: []
notes: "Unchanged per this document: codex remains default for architecture decisions, premise falsification, and review seats (codex + grok, never a Claude subagent as a review seat)."
---

# Maintainer ruling — codex must never orchestrate (2026-08-18)

> as a rule for orchestration, sometimes codex will take over orchestration, that's not desired,
> codex cannot be orchestrator, we must prevent that

## The rule

**Codex is never an orchestrator.** It does not drive a train, own a block, sequence work, dispatch
other agents, or decide what happens next. Its legitimate roles are ADVISORY ONLY:
- **review seat** (conformance / architecture / adversarial mandates),
- **architecture consult / ruling** on a specific question,
- **scoping, premise verification, call-site and blast-radius inventory**,

## Why it drifts, and what actually prevents it

Codex naturally answers a scoping question with a plan — "do X, then Y, then Z". That output is
**advice, and advice does not transfer ownership.** The orchestrator reads it, decides, and owns the
decision. A codex ruling is an INPUT to an orchestration decision, never the decision itself, and
never a licence for codex to proceed.

Concrete prevention, to be applied at every dispatch:
1. **Invoke consults and review seats with `--sandbox read-only`.** An advisory role gets no write
   capability, so it structurally cannot execute its own plan.
2. **Frame every codex prompt as a QUESTION or a RULING REQUEST**, never "drive this", "execute this",
   "manage", or "proceed". Ask what is correct; do not delegate the doing.
3. **Never give codex the ability to dispatch other agents.** Orchestration means fan-out; if a prompt
   would have codex spawning or directing workers, that prompt is wrong.
4. **Codex is NOT an implementer either** (maintainer, same day, superseding the 2026-08-05 note that
   made it the default implementer). It writes no production code. Implementers are `claude-max`.
5. **Sequencing advice from codex is still advice.** Today's parser-relocation consult returned an
   execution order (finish B2, rebase B3, then K1). Adopting it was the orchestrator's decision,
   recorded as such. That is the correct relationship; codex announcing an order does not make it the
   scheduler.

## Unchanged

Codex remains the default for architecture decisions, premise falsification, and review seats
(`codex` + `grok`, never a Claude subagent as a review seat). Orchestration and train-driving belong to
the program orchestrator and to `claude-max` managers.

## Addendum — codex is not an implementer (2026-08-18)

> note the implementer should not be codex anymore

Codex's roles are now ADVISORY ONLY: review seat, architecture consult/ruling, scoping and premise
verification. It does **not** implement, and it does **not** orchestrate. All implementation is
dispatched to `claude-max`. This supersedes the 2026-08-05 reversal that had made codex the default
implementer/fix agent.
