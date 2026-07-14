# `docs/arch/last/` — the consolidated handoff record

> **The cache-poisoning class described here is OPEN and REACHABLE in the landed code, and a
> reachable stack-overflow crash in the shared resolver has NOT been started.** What landed is a
> **checkpoint**: it closes several individually-proven poison sites and repairs a regression the work
> introduced, but it does not close the class. If you are here to continue the work, these documents
> are what you execute from — not a history of what happened.

This directory is the **committed, durable** record of the single-resolution-engine effort. It exists
because the working ledger that drove it lives in `.feedback/` and in scratch directories under
`/private/tmp/`, all of which are ephemeral — a wipe already destroyed one ledger mid-effort and cost
a full re-grounding. Everything of substance from those places has been carried **into** these files.
Treat scratch as gone; treat this directory as the record.

## Read in this order

1. **[`single-engine-cutover-state.md`](single-engine-cutover-state.md)** — the goal, what actually
   landed versus what is merely written, the branches and where the code lives, the remaining
   sequence, the other open defects, and landing hygiene. **Start here.**
2. **[`cache-admission-closure-design.md`](cache-admission-closure-design.md)** — the headline
   remaining deliverable, implementer-ready: the invariant, why the class kept regrowing (the
   root-cause account everyone was working from was **false**), the ruled mechanism (invert scope
   ownership), the type to port from the independent solve, the four known live-production holes, and
   the mandate to **audit rather than patch** — patching sites has failed three times, and the known
   hole set is **not exhaustive**.
3. **[`shared-engine-crash-fix-design.md`](shared-engine-crash-fix-design.md)** — the reachable
   stack-overflow crash, implementer-ready: the iterative heap-worklist rewrite of the shared
   projection primitive, the dual-rail fuse, and the crash regressions that **must** run in a 2 MB
   subprocess because the workspace `RUST_MIN_STACK` **hides** the crash.
4. **[`verification-traps.md`](verification-traps.md)** — four ways this toolchain hands you a **false
   green**, and the two reasoning failures that let a proven bug hide for three review rounds. Read it
   before you trust any "the gate is clean" claim, including your own.

## Conventions

Claims carry their evidence. Statements verified first-hand against the tree say so and cite the file
and symbol. Statements taken from the working ledger or a review leg and **not** independently
re-derived are labelled **(reported, not re-verified)**. Where two sources contradicted each other,
the contradiction is written down rather than silently resolved. A question that could not be settled
appears as an open question with a named way to settle it — never as a fact.

Line numbers drift. **Paths and symbol names are the durable part of any citation**; treat a line
number as a hint, not an address.
