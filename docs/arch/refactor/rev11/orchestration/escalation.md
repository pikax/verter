## Watch the process, not just the block. Escalate rather than grind.

You are the first person positioned to notice that the PROCESS is failing, not the code. Nobody
above you can see a round from the inside. Grinding through another round when the process is the
problem is the single most expensive thing you can do — one block spent 15 rounds and 30 hours this
way without landing.

## Escalate to the program orchestrator the moment you see any of these

- **A leg that did not really run.** Empty, tiny, truncated, no verdict line, or no matching
  `REVIEWED_SHA`. That is BLOCKED, never a verdict — and never a PASS. Four consecutive rounds on one
  block proceeded on an adversarial leg producing 0–1118 bytes and nobody noticed. Check every leg's
  output size and verdict line before you act on the round.
- **Findings are not decreasing.** A review-fix cycle should converge. Two rounds with no reduction
  in finding count or severity means something structural, not more fixes.
- **The same finding keeps coming back in different words.** That is a disagreement about the design,
  not a defect that needs another fix.
- **A leg failed for a reason other than the code** — the SHA moved, it lacked access, it could not
  execute a plant, it ran out of budget mid-analysis. That is a broken dispatch. Fix the dispatch;
  do not record the verdict.
- **A reviewer expanded scope** or a finding would change what the block is for.
- **Rounds 3 and beyond with no clear end in sight.** Say so at round 3. Do not discover it at 15.
- **A rule that cannot be followed as written**, or two rules that contradict each other on your
  block. Report it — the rules are wrong more often than you are.

## An empty leg is a BUG IN YOUR DISPATCH. Diagnose it, do not just retry.

A leg that returns nothing has almost never "found nothing". It failed to run, and the cause is
nearly always the harness or your own prompt. **Re-running it blind wastes the budget a second
time.** Work this list before you re-dispatch — the answer is usually in the first three.

1. **Read the raw output file, not the verdict extract.** The failure is usually stated in the first
   twenty lines: a refusal, a usage error, a missing binary, an auth prompt. `head -40 <the .out>`.

2. **How was the prompt delivered?** codex must receive it on **stdin** (`< prompt.md`). Passed as a
   CLI argument it hangs waiting on stdin and produces nothing at all.

3. **Was the output path writable by that agent?** A sandboxed agent cannot write outside its
   workspace. This has already happened: an adversarial leg was told to write to
   `<agent-rules>/verify/…`, was sandbox-blocked, and silently wrote into its worktree's
   `target/` instead — a directory that then gets deleted. Check the worktree for the report before
   concluding it produced none.

4. **Did the required sidecar exist?** codex at `xhigh` reasoning returns **nothing, silently**, when
   its `codex-code-mode-host` sidecar is absent from `PATH`. An empty leg with a clean exit code is
   the signature.

5. **Did the process die rather than finish?** Check the exit code and the tail. An API 5xx, an OOM
   kill, or a harness teardown all end a leg mid-analysis. That is BLOCKED, not a verdict.

6. **Could the agent do the job it was given?** An adversarial leg with no write access cannot plant.
   A leg pointed at a worktree missing `node_modules`, the plugin dist, or the oracle cache fails
   closed on setup. A read-only seat asked to run tests will report it could not — and that is a
   broken dispatch, not a finding.

7. **Was the prompt too large, or self-contradictory?** A brief that both forbids and requires
   something leaves an agent with nothing safe to output.

**Fix the cause, then re-dispatch.** And say what the cause was when you report — a dispatch bug that
is silently retried recurs on the next block, and on the block after that. Four consecutive rounds on
one block ran with a dead adversarial leg because each was retried without anyone asking why.

## How to escalate

Say what you observed, what you think it means, and what you would do. One short message. Do not stop
work that is genuinely independent of the question; do stop the loop the issue is feeding.

**Do not silently absorb it.** A manager that works around a broken process hides the breakage from
everyone who could fix it, and the next block pays the same cost.

## The chain

1. **You → program orchestrator.** Default. It owns scope, sequencing and landing, and can change the
   process.
2. **Orchestrator → maintainer.** For anything it cannot decide: a scope change, a weakened bar, a
   contested rule, a trade-off between correctness and landing.
3. **Maintainer AFK → codex architect consult.** A recorded ruling stands in for the maintainer.
   Shape it as falsification ("does this violate a named invariant?"), never generation ("what should
   we do?") — an unprimed generative prompt returns a different answer every run. The ruling is the
   decision; cite it afterwards.

Do not skip a step upward, and do not invent a local answer to avoid the step. An unrecorded local
decision is far more expensive to unwind later than the delay of asking.
