# Delivery — code quality, testing, regression prevention

Shared by implementers and reviewers, so both judge a change the same way.

## Code quality

Make the **smallest correct change**. KISS before premature DRY: small duplication beats an incorrect
abstraction. Prefer deletion and reuse over adding concepts.

Every abstraction must solve a current, demonstrated problem. No traits, interfaces, factories,
registries, wrappers, configuration layers or extension points for hypothetical requirements — an
interface needs a real architectural boundary, multiple current implementations, or another concrete
need. Future extensibility is case-by-case, never a default.

No unrelated cleanup, no unnecessary public API, no dependency without a present requirement, no
defensive handling for impossible internal states unless crossing an untrusted boundary, and no
compatibility shims, feature flags, TODO scaffolding or dead paths unless explicitly required.

Follow canonical architecture and existing repository patterns. Code should be understandable locally
without unnecessary indirection; comments explain non-obvious reasons or invariants, never visible
mechanics. Preserve performance — require measurements only on a hot path or where evidence indicates
risk.

**Before completing, ask: what can be removed from this diff without failing acceptance?**

## Testing

A test earns its place by detecting a defect that could plausibly occur. Usefulness matters more than
count.

**Do not add a test that merely asserts** prose, headings or examples exist; that a prompt contains a
sentence already reviewed as prose; that one document duplicates another; that a file changed; or an
internal detail with no behavioural contract. Changing a document, prompt or template does not
require a test. A test for a template is justified only where executable code consumes it — a
renderer, parser, schema, placeholder checker or result validator — and exercises that contract.

Negative tests only for credible external boundaries, distinct failure semantics, or a demonstrated
regression. **One representative case per failure class**; no combinatorial cases without materially
different behaviour. Tests are deterministic: no sleeps, wall-clock races, timing margins or polling
where explicit synchronisation or a controlled clock is available.

## RED/GREEN

Strong evidence that a test detects its intended defect. Apply where that evidence is valuable — not
as blanket mutation of every assertion.

**Required when:** fixing a demonstrated executable bug; adding or materially changing a test for new
behaviour; protecting a correctness, security, concurrency or fail-open invariant; adding a guard or
validator whose purpose is preventing a known failure; the test could plausibly stay green while the
behaviour is broken; the test is itself an important deliverable.

**Acceptable RED evidence:** the test fails on the pre-fix implementation for the expected reason; a
minimal representative defect is planted and it fails for the expected reason; or a compile-fail test
shows the invalid state is rejected. Then restore and observe GREEN. **One demonstration per distinct
behaviour or defect class.**

**Not needed when:** only prose or non-executable templates changed; a refactor preserves behaviour
already covered by trustworthy tests; correctness follows from compilation or existing contract
tests; the change is formatting, naming or mechanical movement; or mutation would be unsafe,
excessively expensive, or weaker than an available proof. Not using it needs no waiver — the manager
records the evidence that does exist.

**Prove a plant applied** — present, unique and new. `perl`, `sed` and `grep` exit 0 on a non-match,
so an exit code never proves a mutation landed, and a verification search hitting a pre-existing
occurrence is a false positive. A green planted run means the plant failed until proven otherwise.

**Stage only your own paths.** `git add -A`, `git add .` and a bare `commit -a` capture whatever a
concurrent writer staged between your `status` and your `add`, and any mutation still applied in the
tree. Add explicit paths, and compare the staged set against your intended file list immediately
before committing — mechanically, not from memory. A commit containing a file you did not write is
the failure, whatever put it there.

## Regression prevention

Use the **lowest-complexity mechanism that reliably prevents recurrence**, proportional to impact,
likelihood and recurrence risk:

- simple correct code, where recurrence risk is low;
- an existing test, when it already discriminates;
- a focused regression test, for a demonstrated behavioural defect;
- type or privacy enforcement, when it fits the architecture naturally and removes meaningful risk;
- stronger structural prevention, for critical or repeatedly recurring invariant failures.

**Do not** introduce newtypes, type-state, sealed traits, closed gateways or extra architecture
merely to make every defect theoretically unrepresentable.

Never land a new name-keyed source scanner: `CLAUDE.md`'s forward-only rule forbids a guard that
greps the tree for a spelled identifier, path or token, `syn`/AST scanning included.

## Health check at every write boundary

**When an implementer or fix agent finishes, run `cargo fmt --all` and
`cargo clippy --workspace --all-targets -- -D warnings` together, as one step, and commit the result
before the candidate is frozen and review is dispatched.**

Together, because run separately they surface as two failures on the same candidate discovered one
after the other, each costing its own cycle. Whole-workspace, not narrowed to the crates you think
you touched.

Before review, because a formatting or lint commit landing afterwards moves the sha the reviewers'
verdicts bind to — the review then no longer covers the candidate that goes forward. Reviewers see
the formatted tree, and the reviewed sha is the one that lands.

This is where the defect belongs: caught at the point the code was written, it never reaches a
candidate. The landing-side check is a backstop for drift a rebase introduces, not a substitute.

## Rebasing

**Rebase onto the working branch at every natural boundary** — after an implementer finishes, after a
fix cycle ends — not only before landing. **Except while anything is being compared or measured
against the branch:** a frozen candidate with lanes running against it, a failure triage comparing
the candidate to a pre-candidate tree, a measurement in progress. Rebasing there moves the subject of
the comparison and invalidates the comparison itself, not merely the freeze. Never mid-slice.

Dispatch the rebase rather than doing it by hand, and prove both equalities — delta-of-deltas byte
identity, and per-file blob identity.

Drift corrupts the question, not just the merge. **Ask what a branch changed with
`git diff <merge-base> <branch>`, never `git diff <trunk> <branch>`** — on a branch that is behind,
the second reports files trunk added as files the branch deleted.

**Rebase integrity is not row equivalence.** A clean rebase preserves the branch's intent, which is
the failure when that intent is stale: patch-ids 1:1 and a clean tree are both consistent with a
branch silently reverting a field its owner corrected upstream. Before a squash, field-diff every
ledger row the branch touches, baselined on the merge-base, and surface a collision where both sides
moved the same field.

## Landing

**Only the program orchestrator dispatches the landing agent.**

**Landing is acceptance. A block does not land until it is accepted, and landing sets it in stone.**
There is no integration-milestone landing with acceptance deferred: a candidate reaching the working
branch is the block's final state, so everything acceptance requires happens first — every acceptance
identifier covered, the fresh review of the final frozen candidate by an agent that has not seen it
returned PASS, and no open finding. A block that cannot meet that does not land; it stays on its
branch until it can.

**A half-landed block is a defect, not a trade-off.** It is not a faster route with a cost attached;
there is no version of it that comes out ahead. Work reaches the working branch owned by nobody, its
obligations surface one at a time as later blocks become their first consumers, and the record says a
block is in progress while its code is what everything else builds on.

The measured cost, from the two blocks that landed this way before the rule: one has fifteen of its
thirty-eight criteria uncovered on the working branch, because two of its four slices never landed —
so closing it now means implementing and landing them, with a four-block train blocked behind it and
a dependent block parked. The other could not be accepted at all: six of its ten scope items open,
two with no evidence in the tree, and its charter forbidding the round that would close them. Both
were reported complete at the time.

Neither was caught by a gate, a review or a conformance check. Every one of those passed. What was
missing was anyone asking whether the block was finished.

**A ready-and-verified report carries the candidate identity, the evidence, the squash message —
subject and body — and the acceptance-coverage mapping.** The manager drafts both at verification,
when what the block did is freshest, and they travel upward with the readiness claim. Landing never
asks for either. Coverage has the same freshness property as the message and more strongly: naming
which criterion covers a change is harder to reconstruct later than describing the change, and the
block is the only party that knows why it made each one.

**The landing agent authors no block-scoped content.** A rebase conflict, a commit message and an
acceptance-coverage mapping are all block knowledge; produced at landing time they are unreviewed,
with the gate about to run on the result. Coverage is the sharpest case: a mapping the checker
invents will always find itself satisfied, because the checker chooses which criterion to point at —
the same trap as a check that enumerates from the source it validates. The block names the
identifier; the landing agent verifies the naming and may refuse. So the landing agent uses the supplied message verbatim and verifies compliance — never
authoring, never rewriting — and either failing cancels the landing and returns it to the block that
owns the code.

The landing agent works in order. Each step gates the next: a failure at any point ends the landing
rather than proceeding with a caveat.

1. **Rebase** onto the working branch if the candidate is behind. **A dirty rebase cancels the
   landing** instead of being resolved.
2. **Run the health check**, after the rebase and before anything expensive. The block ran it too,
   but that result binds to the pre-rebase tree; a rebase onto a moved working branch produces a
   tree nobody has checked, and that tree is what the pre-commit hook judges at squash time. Order
   is cheapest-first so a failure returns the block early:

       cargo clippy --version                 # must match the rust-toolchain.toml pin
       cargo fmt --all --check
       oxfmt --check <the delta's .ts/.js/.mjs/.cjs files>
       cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings
       cargo clippy --workspace --all-targets -- -D warnings
       cargo check --workspace --release

   The pin check gates the rest: a lint result from an unpinned toolchain is not evidence about the
   toolchain CI uses, and it reads exactly like a pass.

   **Always run:** the pin check and both formatters. `cargo fmt --all --check` is a whole-workspace
   check and costs nothing; `oxfmt --check` runs whenever the delta contains any `.ts`, `.js`, `.mjs`
   or `.cjs` file, **wherever it lives** — the pre-commit hook does not care which directory a script
   sits in.

   **Skippable — the three cargo builds only** (wasm32 clippy, workspace clippy, release check), and
   only when the delta, **enumerated against the merge-base rather than accepted as a label**,
   touches none of `*.rs`, `crates/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
   `build.rs`, `.cargo/**` — a docs-only change that edits a rustdoc comment is still a `.rs` file.
   The skippable set is named rather than numbered on purpose: it was once written as a line range,
   and inserting one command into the list silently made a formatter skippable and a release build
   mandatory, inverting both.

   Add `pnpm install --frozen-lockfile` and `pnpm test` only when the delta touches `packages/` or
   the lockfile — an install-and-test trigger, never the formatting trigger.

   Any failure returns the block: this is production source, and a landing agent must not fix it.
3. **Check acceptance coverage.** **A candidate may not land carrying work that no ratified
   acceptance criterion covers.** For every material change in the delta, name the acceptance
   identifier that covers it; uncovered work is bound by a ratified charter or amendment first, or it
   is cut from the candidate. Deferring a block's acceptance does not license landing uncovered work —
   it only moves work nobody owns onto the working branch, where the next block to touch it discovers
   the gap one item at a time. Naming a logical owner is not binding one.
   **Check non-circularity.** A delta that edits the charter, amendment or architecture document
   supplying its own coverage is manufacturing it. Confirm the candidate touches none of the
   instruments its mapping cites.

   **Where a criterion bundles changes with different ratified bases, split it.** A mapping reading
   "these two changes → one criterion, bound by that amendment" invites the reader to infer the
   amendment covers both, and an amendment that expressly excludes one of them then appears to
   authorise it. One basis per row.

4. **Run the block's regression mechanism if the gate does not select it.** The canonical gate's
   exclusion filters are not visible from its verdict, and a block whose only rail sits behind one
   gets a green that is silent about it by construction — a compile-fail suite excluded by package
   and test pattern is the worked example. Run it directly against **the tree that lands, identified
   by tree hash**, and report it. Tree hash rather than commit sha: a squash changes commit identity
   and never tree content, so a sha-based rule forces a redundant re-run for a difference that does
   not exist.
5. **Check conformance.** Each claimed result must exist, reach a conclusion, and bind to the
   candidate's sha, and the supplied message must comply — **in its body, not only its subject**.
   Naming the program, its revision or a block identifier, or a commit type `CLAUDE.md` does not
   list, returns the block. Whoever checks did not produce that evidence — that is what makes the
   check worth anything — and may refuse.
6. **Run the gate.**

**On gate success only:** squash under the supplied message verbatim, then land, then update the
ledger, then **remove the block's worktree, delete the merged branch and prune** — on a successful
landing only, since a returned block needs its worktree intact.

**The ledger commit goes on top of the landed sha, not before it.** Landing is a fast-forward, so a
ledger commit written first puts the candidate one behind, forces a rebase, and produces a new sha —
leaving the row naming a commit the history no longer holds, which is the provenance failure below,
manufactured by the step order that was supposed to prevent it. Land first and the sha is final when
it is pinned. Clearing a block ref that landing deleted is not enough on its own: the row must name a
ref that still resolves and still carries the commit.

**The gate does not run `cargo fmt` or `cargo clippy`.** `CLAUDE.md` keeps them as separate
end-of-change checks, so without step 2 a candidate can pass rebase, conformance and a full gate and
still be rejected by the pre-commit hook at squash time — after the gate has been spent. The release
check earns its place on a class the gate structurally cannot reach: `debug_assert!` gates on a
runtime constant, so a `#[cfg(debug_assertions)]` helper called inside one is `E0425` in every
release build while compiling clean in debug, and the gate's shipped-cfg lane that would otherwise
cover it is currently skipped. It is a compile check and runs no tests — never report it as coverage
of `debug_assert!` behaviour.

**A failing lint run's error set is complete only for the targets it reached.** A clippy run that
stops on the first errors never builds the later targets, so its output is a prefix of the problem,
not an inventory of it — the same shape as a fail-fast gate. Re-run to green; do not treat the first
list as the full set.

**A rebase voids the carry-across.** A candidate that has been reformatted AND replayed onto a moved
working branch differs from the gated tree in two ways, and only one of them is a formatting fix. Do
not stretch the rule to cover the other; gate it.

**The gate's verdict is read from its telemetry, not its exit status.** `completeness` and
`terminal.reached` in the telemetry the runner writes under its target directory are the authority; a
truncated log and a missing terminal summary are corroborating. Do not probe for a working directory
at the repository root — nothing writes one there, so that check reports absence every time and looks
like it is working.

**Skipping the pre-commit hook at squash time is permitted only when the health check above passed on
the exact tree being committed** — re-verify the tree hash immediately before committing rather than
trusting it has held, and state in the report which path was taken and on which hash. The
justification is redundancy: the hook runs the same formatters the health check already ran. If the
health check was skipped, failed, or ran on a different tree, the hook is not a duplicate — it is the
only thing checking — and it runs. Using it to get past a failure is a gate-bypass, not a skip.

**A formatting-only fix does not invalidate a gate verdict**, so a green result carries across it.
That is a statement about the class of change, not about which formatter produced it — it holds for
`oxfmt` over JavaScript exactly as for `cargo fmt` over Rust. Nothing else does: a lint repair that
changes a signature or control flow produces a tree the gate never saw, and its verdict is void.
Verify formatting-only by **recomputing** — apply the formatter to the pre-fix commit and compare
blobs — never by reading the diff and judging it to look like formatting. `fmt` reorders imports,
re-breaks expression chains and adds closure braces, so a whitespace-only test fails on a correct
result and a non-formatting edit riding inside a reformatted hunk reads as formatting.

**Trigger the formatting checks on file EXTENSION, not on directory.** The JavaScript conditional
below fires on `packages/` and the lockfile, which are the right trigger for installing and testing
but the wrong one for formatting: a `.mjs` file anywhere in the tree is formatted by `oxfmt` and
rejected by the pre-commit hook, and a set of probe scripts under `docs/` sailed through rebase,
health check, conformance and a 516-second gate before the hook caught them at squash time — the
exact failure this step exists to eliminate, surviving in the language it was not written for.

**Artifact and binary provenance binds to content — a digest over the artifact's own inputs — never a
commit sha.** Landing rebases and always squashes, so a sha a block recorded moves twice after the
block stopped looking; the record stays well-formed while naming an identity the history no longer
holds, so nothing fails and nothing warns.

`check-results.mjs` does the mechanical half of the conformance check. A block once reported
"adversarial, 20 plants — PASS" for a lane that never ran: no output file, no verdict line, and a
worktree showing one ten-minute build then four hours of silence. Nothing in its account looked
false; it was simply unchecked.

A block's own acceptance evidence is exactly the kind of claim this exists to check — including this
one.

**A clean merge is not a correct merge.** Two changes each correct alone can auto-merge without
conflict and still produce a defect — one narrowing what an inventory collects, the other deriving a
fact from that inventory. Conflict markers do not detect semantic conflict, so integration is
verified on its own.

## Ratification admissibility

**A proof gap may ratify as a recorded unmet obligation. An internal contradiction or a false command
claim may not.** The first states honestly what is not yet established and leaves it visible; the
second asserts something untrue about the artifact or about what a command does, and ratifying it
binds the untruth.

**Naming a Git object identifies a set; it does not resolve one.** A rationale claiming a named
object resolves to a particular member is a false command claim, however reasonable it reads.

## The trap under all the others

**An asserted scope wider than the scope actually examined.** Nearly every stall, false claim and
reopened block traces to it, and it is committed by every kind of participant — implementers,
reviewers, block owners and the orchestrator alike. Treat it as environmental, not as a failing of
whoever committed it most recently.

Its instances look unrelated until named together: a structural-soundness report read as a verdict
about outcomes; a net diff answering a question about intermediate commits; an exit status answering
a question about whether the run completed; a gate's pass read as covering tests its own filter
excluded; a compile-fail fixture proving two types differ while claiming the declaration uses them;
a one-sided absence read as a two-sided fact; a check whose own command line contained the pattern it
matched. Each was true of what it examined and asserted about something larger.

**Capture before delete.** A criterion that measures or rehomes something a pending cutover removes
must have its capture and rehoming land *inside* that cutover, never after it. Land the deletion first
and a counter criterion passes for the wrong reason — nothing remains to charge it — while a
comparison criterion loses the baseline it compares against and becomes unmeasurable, with no failure
to announce either. Before scheduling any new criterion, test it against the deletion set of every
pending cutover.

**A truncated listing that prints nothing reads as "nothing exists".** A survey whose output is capped
supports a negative conclusion only if the truncation is proven not to have hidden the answer — a
branch sweep piped through a line limit across hundreds of branches printed nothing and was about to
be read as work being lost, where a containment predicate found it in three. Prefer a predicate that
answers the question over eyeballing a capped list; as with a match set wider than the defect, the
instrument's shape produced the result rather than the tree's contents.

**A check needs a control on a known-good AND a known-bad run.** A step that can fail may still be
measuring the wrong thing, and a match set wider than the defect inverts under noise without
announcing it: a completeness check matched 25 occurrences on a green run and 16 on a red one,
because passing test names contained the word it searched for. One-sided evidence would have read
that as the green run being worse. Prefer the tool's own machine-readable marker over matching prose
— the same check against nextest's marker gave 0 and 2.

**A waiter exits on receipt OR process-gone, never receipt alone.** A receipt-only poll reports a
dead run as in flight indefinitely, and silent death read as progress costs more than the death.
Detaching a long dispatch is necessary and not sufficient — a detached run has died at 1.4 MB of
output while a smaller one dispatched later survived, so keep dispatch output small: an exhaustive
reading surface, no unprompted context pulls, and the receipt emitted first.

**Open the object before ruling on a claim about it.** A claim travelling upward arrives as a phrase,
and each reader inherits the phrase rather than the thing. A step was ruled vacuous on "no lock record
covers these pins", read at the next level as "no record exists to amend" — the record existed, and
its own text described how to extend it. Three levels handled that claim and none opened the file.
One `cat` ends it at any level, and the cost of not doing it is two reversed rulings.

**One owning statement per mutable proposition. Every other mention is a reference.** A mutable fact
restated in a summary, a justification or a sufficiency argument becomes a second store of that fact,
so a repair applied to one leaves the others asserting the old value — and nothing reports the
divergence, because each reads as prose. A reference may name the owner; it never repeats origin,
value or status. Summary sections survive as crosswalks to owners, sufficiency sections as indexes.

**The unit is a complete truth-bearing proposition, not an attribute of one — and the default action
at a non-owner occurrence is deletion.** Enumerating what a restatement must not repeat (origin,
value, status, mechanism, cardinality) produces a list that grows every round, because each repair
removes the attribute a review named and leaves its siblings asserting the same claim. Delete the
whole sentence, cell or heading instead: a deletion cannot partially retain a predicate, and a
rewrite can — twice did.

**More careful editing is not the remedy.** A block ran an explicit conceptual sweep for one such
claim and still left an independently authored restatement standing; a line-scoped filter cannot see
a block-level strike; a substitution keyed on one of three surface forms of a literal reported success
while missing a site. Reading for the claim rather than matching its string is detection, and worth
doing — it is not prevention.

**A rule stated for others and not applied to oneself is the recurring signature.** The document that
taught this one already required invoking an owner rather than partially restating it, "because
omissions become invisible" — for an external cell, while restating its own facts throughout. Four
findings that looked like a family of related defects were this single one at four scales.

**Amending a record forward is not fabricating one backward.** Adding a row that states what is
pinned as of now is ordinary maintenance; creating a record of a lock that never existed is
retroactive history. They are different acts and the objection to the second does not reach the
first — conflating them blocked the legitimate move for two rounds.

The discipline that catches it is one question, asked of your own claim before anyone else reads it:
**what exactly did this examine, and is that the same as what I am about to say?** Where the answer
is uncertain, the claim is settled by the compiler, a tool, or an independent reader — self-review
does not qualify, because self-review shares the premise.

## One trap worth stating

**A check that enumerates or matches from the same source it validates proves nothing.** A totality
test iterating its own map, a residue check reusing the pattern it verifies, or a drift check
comparing against the recorded sha it is checking, cannot fail for the case it exists to catch.
Verify against an independent oracle: the directory, not the list; the shape, not the pattern; the
live tree, not the pinned baseline.
