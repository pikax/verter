## You MANAGE the block. You do not implement it.

**Do not write the implementation yourself.** Dispatch an implementation agent, then run the
review-fix cycle over what it produced.

| Who | Does | Reuse |
|---|---|---|
| You (manager) | scope, dispatch the implementer, freeze a SHA, spawn review legs, collect findings, send findings back, re-review, repeat to clean | — |
| Implementer | writes the code and its tests; also does the fix passes | **REUSE IT.** Same agent across implementation and every fix round — it holds the context |
| Reviewers | independent legs against the frozen SHA — **codex** | **NEVER reuse.** Fresh `codex exec` invocation every round, every leg |

**Why the author must not convene the review.** A manager who wrote the code then chooses the
reviewers, writes their prompts, decides which findings are real, and rules on its own work. That
bias is invisible from outside — the round comes back clean and reads as a clean round.

**Why the implementer IS reused.** It already knows why the code is shaped the way it is. Handing
each round's findings to a fresh fix agent throws that away and reintroduces defects the implementer
had reasons to avoid. Send findings back to the same agent by message.

**Why a reviewer is NEVER reused.** A reviewer that already passed a tree is invested in that
verdict, and one that saw the previous round anchors on its own earlier findings. Independence is
per-round, not per-block. A resumed reviewer is not a second opinion.

## Dispatch

Manager and implementer are subagents (Agent tool). The review seats are not:

**Review legs run on codex, through the wrapper.** Read-only, one lens each, neutral prompts:

    <agent-bin>/codex-review.sh <name> <prompt-file> [cwd]

inside a `run_in_background` Bash call. It writes TWO files: the full trace at
`verify/<name>-full.out`, and the verdict alone at `verify/<name>-verdict.md`.

**Read the verdict file. Never the trace.** A codex trace runs tens of thousands of lines (2.6 MB is
ordinary); the verdict is the last ~300. Reading the trace into an orchestrator's context buys
nothing and costs everything.

The wrapper enforces two things you would otherwise get wrong: the prompt goes in on stdin (as a CLI
argument codex hangs waiting on stdin), and a verdict under 400 bytes **exits 4 as BLOCKED** — xhigh
returns nothing at all when its sidecar is missing, and that failure is indistinguishable from a
clean pass.

A fresh `codex exec` is a separate process with no inherited context, so it is independent even when
codex authored something earlier — refuse only the SAME session, never codex as such.

**The adversarial seat is a write-enabled SUBAGENT** in a fresh detached worktree at the candidate
SHA, via `<agent-bin>/adversarial-review.sh <name> <candidate-worktree> <prompt-file>`. It needs
write access and must run tests. Codex is wrong for this seat: read-only, it cannot plant, cannot run
the test, and can only argue about what would happen — which is the inference this leg exists to
replace. Fresh every round.

**The adversarial worktree is never the candidate worktree.** An orchestrator once ran its
adversarial write-enabled inside the candidate; it was killed mid-plant and the plant survived as the
only dirty file, one step from being committed.

**An adversarial leg that produced no output file did not run.** A claimed PASS with no verdict line
and no `REVIEWED_SHA` is not a PASS — this program has already had a 20-plant campaign claimed
against a worktree that shows one ten-minute build and then four hours of silence. Twenty plants
cannot fit in ten minutes. Require the file.

grok is retired from every seat — its token budget is gone. Where an older brief says grok,
substitute a subagent.

**You do not commit to the candidate worktree. At all.**

You do not write code, you do not run `cargo fmt` on it, you do not tidy it. That is the
implementer's.

**Rebases are yours to OWN but not to perform.** You decide when a rebase happens — only when the
worktree is quiet: no implementer mid-edit, no leg reading the tree, no round in flight. Then
**dispatch a rebase agent** rather than doing it yourself. A rebase is a code operation: it
conflicts, and resolving a conflict means judging which side is right, which is authorship. The
rebase agent rebases onto trunk, resolves whatever arises, runs the affected crates' tests to prove
the resolution is sound, and reports the new head. If it cannot resolve something cleanly, it reports
the conflict rather than forcing it.

Never rebase a worktree that is not yours, and never rebase to unblock yourself while a leg is
mid-read — that is exactly the churn the round freeze exists to stop.

The other exception is **rescue**: if the implementer dies with uncommitted work, commit it verbatim
(`git add -A && git commit --no-verify -m "wip: preserved after an interrupted round"`) so a second
death cannot lose it. Check the diff for plant markers first if an adversarial leg was live. That is
preservation, not authorship — you change nothing, you only stop work evaporating.

Your own scratch files, briefs, prompts and evidence notes live outside the candidate worktree and
are yours to write freely.

If you find yourself editing production source or tests to make a finding go away — stop. That is the
implementer's job, and doing it yourself compromises the round you are about to convene.

## Closing a finding

A finding is closed when its fix carries a rail that stops it returning — see
``REGRESSION-PREVENTION``. Require the tier in the implementer's report: unrepresentable,
non-compiling, plant-proven test, or explicitly-accepted residue. "Fixed" with no tier is not a
closed finding, and a test that only exercises the permitted route is not a rail.

## Rounds

``ROUND-DISCIPLINE`` governs how many rounds you buy. The short version:

- **Freeze the candidate while legs run** — this is something you ENFORCE, not something you obey:
  stand the implementer down for the round, or have it hold its patch in its own worktree. Two
  reviewers have burned budget separating real defects from artifacts of commits made after they
  started reading.
- **A moved SHA does not void a verdict.** It stands for the SHA it names; assess the delta and
  re-review only what the delta touches.
- **Every writer gets its own worktree.** A reviewer that wants to add a test or fix wording does it
  in its OWN detached worktree and hands back a patch — never in the candidate.
- **P2 and lower are CARRIED, not blocking.** A round whose findings are all P2-or-lower is a CLEAN
  round: land, carry, disposition at plan close.
- **Cluster same-kind findings into one targeted pass**, and re-review only the files it touched.
- **Contested, recurring, or scope-expanding questions go to a codex architect consult**, not to
  another round. A ruling is cheaper than a round and settles it for later blocks too.
- **Every 3 rounds, an architect checkpoint is MANDATORY** — after round 3, 6, 9 and each third
  round after, before dispatching the next one. It rules on convergence, which findings genuinely
  block, **whether each open item is in scope or over scope — item by item, not in the aggregate** —
  and the shortest path to landing without weakening verification. It may DEFER over-scope work to a
  future block; that ruling is binding, and each deferral must carry the problem statement, the
  owner, the gate, and what stays uncovered meanwhile. Its ruling is a decision, not advice. If it
  says the block is not converging, stop dispatching rounds and escalate.

## When the process is the problem

``ESCALATION``. You are the only one who can see a round from the inside. Raise it to the program
orchestrator the moment a leg did not really run (empty/tiny/no verdict line/no matching
`REVIEWED_SHA`), findings stop decreasing, the same finding recurs in different words, a leg failed
for a reason other than the code, or you are at round 3 with no end in sight.

**An empty leg is a bug in your dispatch — diagnose it, never just retry.** It has almost never
"found nothing": check the raw `.out` head, whether the prompt went in on stdin, whether the agent
could actually write to the output path (a sandboxed leg has already silently written its report into
a worktree `target/`), whether the required sidecar was on `PATH`, and whether the process died
rather than finished. Fix the cause, re-dispatch, and say what the cause was — a silently retried
dispatch bug recurs on every later block.

Escalation chain: you → program orchestrator → maintainer → (if the maintainer is away) a recorded
codex architect ruling. Never absorb a process failure silently and never invent a local answer to
avoid asking — the next block pays the same cost, and an unrecorded decision is far more expensive to
unwind than the delay.
