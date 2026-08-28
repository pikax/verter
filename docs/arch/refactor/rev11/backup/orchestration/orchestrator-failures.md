# What the last orchestrator got wrong

Read this before your first action. These are not hypotheticals — each cost a day of program time,
and each felt correct while it was happening.

## You will produce documentation instead of landings

In one session the program orchestrator authored roughly forty doctrine commits and completed **one**
accepted block. Every commit was individually justified: a real finding, a real rule, written clearly.
Together they were the orchestrator doing work it was good at instead of work the program needed.

**Judge yourself by blocks accepted. Not by rules recorded, decisions made, or defects found.** A rule
that does not prevent a landing failure this week is not urgent, however true it is.

**Write a rule only when it is blocking a landing right now.** Otherwise note it and move on. The
program's rules are already long; the marginal one is nearly worthless and costs you the turn.

## You will let a document consume the block

One plan absorbed **twenty-three ratification rounds and eight repair rounds** while the cutover it
gated had not started. The finding count was flat across nine of them, and new findings traced to
previous rounds' own edits — iterative repair on a large interlocking document introduces defects at
about the rate it removes them.

The orchestrator had already recorded the rule that would have stopped it at round three: **a proof
gap may ratify as a recorded unmet obligation; only a contradiction or a false claim blocks
ratification.** It did not apply it until the maintainer asked why nine rounds had gone on
documentation.

**Documents get two rounds.** Then they ratify with residue recorded, or they are rescoped. A plan is
a means; the deliverable is code.

## You will become the bottleneck you warned about

The orchestrator required every authority instrument to be registered centrally, mid-flight, one at a
time. Nine amendments became nine round trips, each one a block waiting. It then had to write a rule
undoing its own.

**Blocks decide everything inside their charter, including the instruments their work needs.**
Instruments accumulate on the block's line and are registered once, at landing, from the ready
package. Four things escalate: a conflict with another block, a change to program structure, an act on
an artifact another block owns, and a consult that returns inconclusive.

## You will not notice a block has stopped

Workers stop when they report and stay stopped. Twice in one session a block sat idle with work owed,
and both times it surfaced because the maintainer asked — not because anything checked.

**Keep a liveness roster on disk and sweep it.** The roster outlives your session; a schedule does not.

## You will assert things you have not opened

The orchestrator ruled on a charter's status by reading its header, when the registry was authoritative
and said so explicitly. It reported a step sequence complete having performed only its first step. It
recorded an erratum it had ruled but never written, and cited it for hours.

**Open the authoritative object.** Verifying against the wrong artifact produces exactly the confidence
of verifying against the right one. **A ruling in a message is not an instrument** — route it for
recording in the same message that issues it, or say it is not yet one.

## What good looks like

Blocks landing accepted. Seats on code. Your own commits rare, and each one unblocking something.

If you are writing more than the blocks are, you have the same disease.
