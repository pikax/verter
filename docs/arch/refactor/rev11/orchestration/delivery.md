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

**A plant must assert the SURVIVORS, not only the victim.** Proving a mutation is present, unique and
new checks the victim — and a restore that silently failed leaves a tree where the plant correctly
finds nothing to change, so victim-side verification passes vacuously. One capture staged its restore
into the index rather than the worktree, so the run re-executed the pre-fix tree while logging a clean
single-line kill: a red result that looked exactly right for the wrong reason. Count the untouched
parts of the fix too; that is what separates "I killed one line" from "I am testing a different tree
than I think I am."

**Record a discarded run in the evidence.** A near-miss that is deleted teaches nobody, and the next
reader repeats it.

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

**A grandfathered scanner may be maintained, not extended.** "Retained as-is" means no one is
required to remove or replace it; it does not freeze its contents against a rename. Updating a
forbidden set so the same coverage survives a renamed symbol or pipeline is maintenance and needs no
instrument. Adding names so it catches something it did not catch before is extending a name-keyed
scanner, which the forward-only rule bars — and stating which of the two it is doing is what an
amendment touching one owes.

**A proof that deletes every member never tests deleting one.** Aggregate deletion shows a check
notices total absence; only per-member deletion shows it binds each member. Ten obligations living
inside one sentence meant any surviving sibling kept the claim satisfied, so "delete all ten and it
refuses" was true and tested nothing that mattered.

**Derive a gate's universe from every section it governs, not from the one that prompted it.** A gate
derived only from the section whose defect prompted it left two other sections underived, so several
rows — including the one an entire acceptance turned on — carried no claims and were bound by nothing.
This is the ruling-scoped-to-its-instance defect applied to a gate, and it has now been committed by a
block that had recorded the rule minutes earlier.

**A count answers who calls it; the criterion asks whether the guard can fire.** A mechanism was sized
by counting a sink's consumers while the feature graph went unchecked, and the guard could not fire at
all: the test graph enabled the very feature it was gated on, so the fixture compiled with the sink
exposed. Different questions, and only the second establishes the criterion.

Never land a new name-keyed source scanner: `CLAUDE.md`'s forward-only rule forbids a guard that
greps the tree for a spelled identifier, path or token, `syn`/AST scanning included.

## Dispatch preflight — establish at the start what is otherwise found at the end

**The preflight runs as a high-effort architecture consult, not as the block's own survey.** It is
guidance the block builds on, produced by someone who is not about to implement it, and it is where
the decomposition into sub-blocks is decided.

Most rounds are spent rediscovering facts that were knowable before implementation began. A block
establishes these **before its first line of work**, and reports them in its first message:

- **Ancestry.** What its base actually contains, by `merge-base` and `--contains` — not what a plan
  says it accumulates.
- **The authoritative artifacts.** Which document is authoritative for each property it will assert,
  opened, not inferred from a status line or a neighbouring summary.
- **Its acceptance surface.** Every criterion it must satisfy, enumerated from the ratified source,
  with the mapping built as work proceeds rather than assembled at ready.
- **Uncovered work.** Anything in scope that no criterion covers, raised then — not after a gate.
- **The instruments it will need.** Named up front and authored in one pass, not one per finding.
- **Its gates.** For each criterion, whether the gate exists, and whether it establishes the property
  or merely something adjacent to it.

None of this weakens a block. It moves the same findings earlier, where they cost a paragraph instead
of a round — and every one of these was found late at least once.

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

**Verification is three-sided.** A step must fail when it should, pass when it should, **and be
incapable of reporting success while its subject is broken.** The third is not implied by the first
two: a generator whose transcript write failed left the previous file in place, so a hash comparison
saw equality and read it as success — the failure invisible precisely because nothing changed. Ask
what your check does when its subject is absent, stale or unwritten, not only when it is wrong.

**When a finding recurs one rung higher, close the range rather than the instance.** A fix that
handles the reported case and leaves the next one is a ladder with no top; the recurrence is evidence
the property is unbounded. Sweep the range and name the residual bound in the test, so the limit is a
declared constant rather than wherever the last review happened to stop.

**Errors that all point the same way are pressure, not scatter.** Independent mistakes scatter; a
sequence of corrections that are every one optimistic — or every one pessimistic — is evidence of a
force acting on all of them. Stop correcting them individually and find it. In one block every
intermediate coverage number was optimistic in the same direction, and the pressure was the pull
toward reading a green run as evidence for the property it was cited for.

**A gate that asserts presence where the criterion claims a property is deficient, not the criterion.**
Narrowing the criterion to match what the gate measures relaxes a ratified requirement because the
mechanism fell short. The costumes this takes: presence standing in for a property, a proxy standing
in for the property, fixtures standing in for a universe, and a cross-check that does not exist.

**A ratifying act names a candidate sha.** "This evidence set", "the current analysis", "as it stands"
are descriptions, not referents: the next commit invalidates them, and an act whose referent has never
landed pins nothing anyone can check. One act scoped that way expired itself — the only commit after it
was the one recording its own expiry boundary, which fired on its own bytes while changing no
disposition it had ratified.

**An enumeration you truncate for readability is an enumeration you did not run.** Truncation
converts a set into a sample silently, and a conclusion drawn from the sample reads exactly like one
drawn from the set — a sweep cut at a column width missed an occurrence a quarter of the way into a
long line and wrote "different claim, out of scope" about a claim it had not finished reading.

**Whether work accumulated on one line is a git fact, not a policy statement.** "It accumulates" said
as intent reads identically to "it accumulated" said as observation, and two branches described as one
line turned out to be siblings sharing only their fork point — with the criteria a precondition named
absent from the base that precondition was handed to. Check ancestry before relying on it.

**A rebase invalidates every claim a document makes about files it does not own.** Rebase-integrity
proofs cover blobs and deltas, and structurally cannot see a claim *about* a file whose content moved
underneath it: a script grew sixty lines so every line citation into it displaced, and an upstream
re-pin left a document describing an old-and-old pairing that had become a hybrid. Both were correct
when written and false when read. So post-rebase citation re-verification is its own step, and a
citation into a file the block does not own is a behaviour or a function identity, never a line
number.

**A finding dispositioned but not briefed is a finding lost.** Carrying the questions forward without
the open findings loses everything the earlier rounds bought — one round's flag that a named function
did not exist vanished because the next brief was scoped to its six questions.

**At freeze, re-derive every digest the candidate quotes — never carry one forward.** A digest that
was right an hour ago describes a reference that may have moved since, and the drift is in what the
work is measured against rather than in the work, which is the one class a clean self-check cannot
surface.

**A test that would apply is not a ruling that has applied.** A decision reached in correspondence is
a decision, not an instrument, and a block citing it as ruled is citing something that does not
exist. Only the record settles which. Route a ruling for recording in the same message that issues
it, or mark it explicitly as not yet an instrument.

**Enumerate the whole range and disposition every member.** A curated list omits the case nobody
thought of, and a reviewer then has nothing to catch. Bounding a range from both sides and giving
each member an explicit disposition is what made a false entry findable — completeness is what makes
an artifact falsifiable.

**Correct a document where its pin is authored, not where its bytes are convenient.** A branch that
inherits a registry byte-for-byte and authors none of it cannot correct what that registry pins: it
would hold new bytes against the old digest it inherited, a live mismatch. Changing the document and
its pin together, on the side that authors the pin, leaves every copy self-consistent at every point
— old-and-old before the rebase, new-and-new after.

**When a cost argument and a consistency argument disagree, the cost argument was never the
criterion.** One rebind versus two is a heuristic; a digest that does not match its bytes is a defect.
Reach for the count only where correctness does not decide.

**An act pins the content it ratifies — named files and their digests — never a tree sha.** A tree
sha covers everything, including the record of the act itself, so recording the act changes the tree
and unpins it. That is unavoidable by ordering: the consequence of an act is an evidence change, so
an act pinned to a whole tree that must record it unpins itself the moment it is recorded. Pinning
the ratified set instead closes the loop, because recording the act elsewhere in the tree cannot
touch it.

**A field checked only for non-emptiness can be destroyed invisibly.** Replacing a long ratified
scope with a short string passes every automated check, because nothing tests what it says — so an
edit to such a field states its requirement as a **prefix relation**: every byte of the existing value
preserved, the addition appended. That is citation-by-identity applied to content rather than
location, and unlike quoting the current text it cannot go stale as the field grows.

**A table is read as a shape before it is read as rows.** Where one row's healthy value differs from
every other's, say so beside it — a reader scanning a column of zeros reads the one as the defect.

**Never infer a value is protected because a sibling copy is.** One block's charter digest is held in
two places: the registry copy is hashed against the document on every run and fails loudly when
stale, while the ledger copy's correspondence check is gated on a set of statuses the block is not in,
so a well-formed but wrong digest passes. Proved both ways — a shape-invalid digest fails, a
shape-valid wrong one does not. A green run over two bindings is evidence about the one that is
checked.

**A gate's coverage can depend on the subject's status.** Ask which checks are active for the state
the thing is actually in, not which exist.

**A pin fixes drift, not contradiction.** Pinning content guarantees the bytes have not moved; it
guarantees nothing about whether those bytes agree with the act. One act named an execution model
while the document it pinned still described that model as unnamed and the choice as later work's —
true when written, left in the present tense afterwards, and the pin perfectly maintained throughout.
Before issuing or re-pinning, read the pinned text and confirm it says what the act claims it
ratifies.

**A moved pin is not a process failure.** When review finds a defect, the pin moving is the correct
consequence of fixing it. Treating the movement itself as the fault suppresses the fix.

**Ratify last** remains the ordering — evidence, freeze, act — but it is no longer load-bearing on
its own. An act issued before its own consequence is recorded either expires when the recording lands
or contradicts the evidence it pins; that happened twice before the pinning was fixed, and a third
time when the instruction to fix it moved the tree.

**An act whose own consequence must be recorded in the evidence it pins can never be self-consistent.**
Record the consequence first, freeze, then pin. Re-pinning is cheap; an act that cannot be made
consistent is not.

**A narrow grant with a drifting referent is the worst pair:** it expires early and cannot be checked.
Narrowness is right; it has to be pinned.

**A cutover may run the new path alongside the legacy one inside a sub-block. The block does not
close until the legacy path is deleted and the cutover is clean.** Parallel paths are a reviewable
intermediate state, not a shipping one: they let a sub-block be judged on the new path working rather
than on everything moving at once. Carrying both past the block's own close is the dual-path outcome
the architecture rules forbid, so the deletion is the block's last sub-block at the latest, never a
follow-up.

**A crate-scoped run is blind to tree-scanning guards.** Testing the crates you touched reports green
while a guard that walks the whole tree is red, because your crates are not where it looks. Run the
tree-scanning guards on any change that moves, deletes or relocates files, whatever its crate
footprint — one relocation would have shipped with two red.

**Take the baseline before implementing, in a separate worktree.** A pre-implementation run tells you
which failures were already there; without it every carried failure is attributed to your change. One
relocation started from 176/2 already red and finished 178/0 — a claim to be better than where it
started, which is only available because the baseline existed. A separate worktree keeps in-flight
work untouched and keeps the shared stash out of it.

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

**Fix the threshold before you know the count.** A decision criterion chosen after the measurement
justifies whichever answer arrived: thirteen of eighty-three needing a carve-out settles a question
only because ~15 was the threshold before the count existed. Pre-register the signatures a run must
not produce, and report them as signatures that did not fire.

**Never let an error branch print the word a pass prints.** A `grep -qF` sampling a line beginning
with a dash read it as an option, errored, fell to the `else`, and the script printed "ok" — a failed
check and a passing check emitting the same word. Pass the pattern after `-e` or `--`, and make the
error path say error. This is the filter-matching-nothing family one step worse: there, success and
emptiness coincide; here, failure is reported as success.

**Match the brief to what the worker can actually do.** A tool that cannot commit inside a linked
worktree is told to stop at edits and leave the tree dirty, with the dispatcher committing — rather
than being asked for something it will fail at silently. An earlier pass died mid-flight with
uncommitted work nobody knew existed.

**A check needs a control on a known-good AND a known-bad run.** A step that can fail may still be
measuring the wrong thing, and a match set wider than the defect inverts under noise without
announcing it: a completeness check matched 25 occurrences on a green run and 16 on a red one,
because passing test names contained the word it searched for. One-sided evidence would have read
that as the green run being worse. Prefer the tool's own machine-readable marker over matching prose
— the same check against nextest's marker gave 0 and 2.

**A long external consult runs backgrounded, and the dispatcher does not fully yield.** A foreground
consult is killed at roughly ten minutes, so anything past that dies with no receipt — and the death
presents as a prompt or model problem, which is how several were misdiagnosed. The dispatcher is
notified on completion, so it must stay alive to receive that: a full yield ends the turn and the
notification lands nowhere.

**Never require a lane to emit its receipt before it has one. That instruction is retired.** It was
adopted so a dying run would still produce a receipt, and it produced five distinct defects: a receipt
echoed from the prompt, a genuine-but-not-final receipt mid-run, and finally three own-lane receipts
that disagreed, which the shared validator correctly refused as inconclusive. Across two runs the
provisional verdict was wrong in both directions — provisional pass to final fail, then provisional
fail to final pass — so it was not even a conservative bound and carried no information.

**The protection against death-before-verdict was always small output and waiting on process exit**,
both of which were already required. Four patches were written for this, each aimed at the reader
when the source was the instruction: **when a rule keeps producing new failure surfaces, retire the
rule rather than hardening its readers.**

**A receipt is final only once the producing process has exited.** Wait on the process, then take the
last own-lane filled receipt.

**A waiter exits on receipt OR process-gone, never receipt alone.** A receipt-only poll reports a
dead run as in flight indefinitely, and silent death read as progress costs more than the death.

**A template for evidence, placed in the input, becomes indistinguishable from the evidence once the
input is echoed.** A receipt block carried in a prompt so a dying run still emits one is echoed back
verbatim, so a waiter matching on its markers fires at second zero and hands back the empty template
as a verdict. Any marker appearing in both the prompt and the answer cannot discriminate between them
on its own: require a FILLED result, reject the placeholder, and read the LAST matching block rather
than the first.
Detaching a long dispatch is necessary and not sufficient — a detached run has died at 1.4 MB of
output while a smaller one dispatched later survived, so keep dispatch output small: an exhaustive
reading surface, no unprompted context pulls, and the receipt emitted first.

**Opening AN object is not enough — open the AUTHORITATIVE one.** A document's own header is not
what classifies it. A charter read as unratified because its status line said DRAFT was ratified in
the registry, which nobody opened; the registry says in terms that a charter is never classified by
its own status prose. Ask which artifact is authoritative for the property you are asserting, then
open that one. Verifying against the wrong artifact produces the same confidence as verifying against
the right one.

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

**When a count and its items disagree, correct the count.** A lead-in saying five obligations above
four rows is fixed by writing four, never by authoring a fifth. The tempting repair makes the document
self-consistent by adding content nobody wrote, and a worker optimising for consistency will take it
unless told not to.

**Four kinds of slot are exempt from the deletion remedy: one that carries a VALUE, one that
identifies a ROW, one that states the DISTINCTION a status depends on, and one that states the
CONDITION A GATE FIRES ON.** An abort trigger's text *is* the trigger — a condition nobody can read
cannot fire. Six of eight triggers were reduced to bare section pointers, and the one that read "the
harness is deleted only after its conversion is in place" became unreadable; a worker then deleted the
harness and twenty-four differential cases with it. This bound has now cost a document defect, a false
status, and a lost test suite, each time by the same remedy applied one category too widely.

**A cardinality control cannot detect a gutted cell.** The pre-registered signature was that the eight
trigger rows differ in count; it reported eight of eight preserved, and six were hollow. The same
session had already verified one table by content after this class bit there, and then verified
another by count — the rule did not transfer between two tables in one sitting.

**A verification that does not name its tree is not reproducible. Read a production claim at the
production ref, and state the ref in the claim.** A defect was reported against a site that exists
only on an unlanded branch; three readers each believed they had verified it, and the surrounding
steps were verified correctly at the production ref, which is what made it survive. The defect was
real at a different site — but nothing in the report said which tree any step came from.

**Correcting a claim is not the end of it — enumerate what was derived from it while it stood.** One
unsourced link produced three artifacts before anyone checked it: the escalation that framed it as
production, a ledger row that inherited its scope, and an implementation brief naming symbols that do
not exist. Each was written by someone acting correctly on the previous one. The claim is one
artifact; the instruments built on it are the expensive ones, and they do not self-correct when it
does.

**Evidence produced BY the implementer is not a review, however good.** A package can carry a failing
test seen failing, a kill matrix, discrimination in both directions and a discarded-run record, and
still contain no independent verdict. Check a package for what it does not contain — the request that
produced it is where the omission usually lives.

**A chain with three sound links and one unsourced link reads as verified**, because every check
anyone runs lands on a link that holds. Two readers each verified the steps the other had not, and
neither checked the one step neither had. Verify the step nobody has named a source for, not the step
you find easiest to confirm.

**The hinge is where the damage lands, not where the fix goes.** A fix belongs at the site that can
tell the causes apart. An arm returning a result on an empty value may be correct for a genuine miss
and have no information to distinguish it from a budget trip, so fixing it there is fixing the wrong
end.

**Check the sibling arms before concluding a mechanism is absent.** A file-scoped search of a seam
cannot see correct handling on a neighbouring arm of the very branch that fails, and every enclosing
scope looks right. One marker was applied on a function's error arm and skipped by an early return
seventeen lines above it — an asymmetry inside one match, not a missing capability, which is cheaper
to fix and far harder to defend.

**A contract that cannot be tested is a finding about the production code, not about the test.** A
row is re-scoped when the ROW is wrong, never when the CODE is — otherwise an implementation edits its
own acceptance criteria by being deficient, and re-scoping ratifies the violation the criterion exists
to prevent.

**A `.flatten()` over a nested Option is a discarded distinction, not a missing one.** The type carried
the difference between a driver's disposition and a genuine not-found; the code erased it
deliberately. That is smaller to repair and much harder to defend than an absent capability.

**An untyped loop bound is unrepresented, not unobservable.** The first needs a type; the second needs
an instrument. Do not answer one with the other.

**A true fact with no witness is not a discharged row.** What discharges a row is the instrument, not
the observation — a fact nothing asserts is one refactor away from being false with nobody notified.

**"Became assessable" is a third state, distinct from moved and from not moved.** Collapsing it into
moved launders evidence into proof; collapsing it into not moved invites redoing work already done.

**Report movement backwards separately, never netted.** A net direction hides the rows that went the
wrong way, and those are the ones worth knowing about.

**A per-case scan cannot see a per-case assertion made centrally.** Counting assertion references
inside each test body found two and nearly reported twenty-two tests vacuous — the assertion lived in
a shared helper and applied to all of them. Follow one case through its helpers before reporting a
population of missing assertions. Same false negative as counting table rows instead of reading cells:
both mistake the shape of the search for the shape of the code.

**A gate that can decline to fire and say nothing is a gate you do not have.** One assertion's
applicability keyed on a thread name and returned silently when absent, so a single test-runner flag
could have disabled every witness assertion in a suite while everything stayed green. Its contingency
was a knob nobody would think to check. Make the decline loud: this is the same class as a wrong
result routed through an error path.

**Retiring one arm of a differential silently strengthens every assertion written against its
normalised view.** Those assertions were written under the comparison layer's equivalences and
exclusions — one read as "a manifest consult exists, under the ratified boundary difference" and
became "this exact variant exists" the moment that layer went. The conversion preserved the assertion
and dropped the normalisation it depended on.

**The failing one is the only assertion that announced itself.** A lost normalisation can equally make
an assertion pass vacuously or pass on the wrong fact, and the surviving greens are silent on the
question because they never exercise the changed arm. Enumerate the cross product — every equivalence
and exclusion the deleted layer applied, against every assertion converted out from under it — rather
than resting on the tests that still pass.

**Ask for the deletion inventory, not just the delta.** Net removal proves a relocation is not a copy
and says nothing about whether what was removed was supposed to survive. Reviewing a change by its
line counts and its passing gates leaves that question unasked.

**Three kinds of slot are exempt from the deletion remedy: one that carries a VALUE, one that
identifies a ROW, and one that states the DISTINCTION a status depends on.** A clause justifying a
status is not a restatement of the section that defines the terms — it is the status's reason, and
removing it turned an honest UNMET into what reads as a denial of an established fact.

**The deletion remedy applies to PROSE ASSERTIONS, never to a value-carrying or identity slot.** A
table row's key cell is not restating the section that defines it — it *is* the row's identity, and
reducing it to a pointer makes rows indistinguishable, two of them literal duplicates. Frontmatter
naming what a document binds or supersedes carries machine-readable data, and a consumer reading a
section pointer learns nothing about which files lost normative role. Applying the remedy there
introduced defects a complete inventory had correctly found nowhere.

The complete list was right; the remedy needed this category distinction. When teaching it, carry the
case where deletion IS correct alongside it — without a counter-example a worker generalises to "stop
deleting", which is the opposite error.

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
