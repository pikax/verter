# Orchestration

How program work is implemented, reviewed and landed.

Reliability comes from clear ownership, evidence, deterministic tooling and calibrated review — not
from repeating rules or maximising ceremony.

## Runtime modes

Two legitimate ways to host the Claude side. The operator picks per program; neither is the default.

- **Split-pane Agent Team** — levels 1–2 as team lead and named teammates, workers as subagents.
  Visible sessions in one window; teammates cannot run background subagents and do not survive
  `/resume`.
- **Supplied CLI pool** — the operator supplies external Claude CLI sessions and accounts, and owns
  their liveness and cleanup. Multi-account routing and visible named terminals; sessions outlive
  the parent.

External tools (Codex, Grok) are external processes in either mode.

## The four levels

Seats are roles, not runtimes. In team mode only the top two form an Agent Team. **No nested Agent
Teams** in any mode.

| Level | Who | Owns | Writes code |
|---|---|---|---|
| 1 | Program orchestrator — team lead | DAG, sequencing, capacity, landing order | no |
| 2 | Block orchestrator — named persistent teammate | charter, scope, architecture, slices, decisions | no |
| 3 | Manager — ordinary subagent, one per block | delivery: dispatch, review lanes, finding closure | no |
| 4 | Workers — nested subagents or external CLI | implementation, review, specialist analysis | implementers only |

Every layer owns a distinct decision. A role that only relays text is removed.

## Authority order

1. Direct user or maintainer instruction.
2. Canonical architecture and accepted decisions.
3. Block charter and acceptance criteria.
4. Current block state record.
5. Orchestrator decisions.
6. Reviewer findings and suggestions.

No model is authoritative because of its identity. Codex is the preferred architecture agent, but
every ruling cites the relevant invariant and concrete repository evidence. Ambiguity or conflict
escalates; an agent may not silently invent architecture or override the user.

## The documents

| File | What it governs | Injected into agents |
|---|---|---|
| [roles.md](roles.md) | ownership boundaries, communication contract, block state | by reference |
| [review.md](review.md) | discovery → closure → acceptance, calibration, routing | by reference |
| [delivery.md](delivery.md) | code quality, testing, regression prevention | by reference |
| [prompts/](prompts/) | the dispatch prompt for each role | yes — these are the runtime prompts |
| [design-notes.md](design-notes.md) | why these rules exist, and the failures behind them | **never** |

**Runtime prompts carry only role boundary, inputs, actions, output contract and stop conditions.**
Rationale lives in `design-notes.md` and is never injected. Do not inline a document into a brief;
point at a path.

## Tooling

Deterministic mechanics belong in tooling, not in prompts:

    node scripts/orchestration/check-results.mjs <results-dir> <sha> <lane>...

Each result must be structurally sound and bound to its lane and reviewed tree. The results directory
is named for that sha, so a leftover file from an earlier freeze cannot answer for a lane that
produced nothing. Exit 0 sound, 1 otherwise, 2 usage — `<sha>` must be the full 40 characters even
when that directory is named with the short form. Absent, truncated or inconclusive is BLOCKED.

    rust-lock.sh <name> -- <command>

Every heavy Cargo workload — gate, build, mutation run — goes through this semaphore. It bounds
concurrent builds host-wide and is re-entrant. **Host-provided, on `PATH`** — not shipped here;
preflight `command -v rust-lock.sh` before the first dispatch that needs it, so a missing tool
fails at dispatch, not mid-run.

## Placeholders

`<repo>` is the repository checkout and `<worktree-root>/verter-<block>` a block worktree. These are
operator environment, not project source.
