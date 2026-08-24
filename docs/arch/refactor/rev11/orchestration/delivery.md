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

## Independent landing verification

**Before a candidate lands, someone who did not produce the evidence checks that it is real.** They
verify that each claimed result exists, reaches a conclusion, and binds to the candidate's sha — and
they may refuse.

No named role, no checklist. `check-results.mjs` does the mechanical half; the half that matters is
that whoever runs it is not the author. A block once reported "adversarial, 20 plants — PASS" for a
lane that never ran: no output file, no verdict line, and a worktree showing one ten-minute build
then four hours of silence. Nothing in its account looked false; it was simply unchecked.

A block's own acceptance evidence is exactly the kind of claim this exists to check — including this
one.

**A clean merge is not a correct merge.** Two changes each correct alone can auto-merge without
conflict and still produce a defect — one narrowing what an inventory collects, the other deriving a
fact from that inventory. Conflict markers do not detect semantic conflict, so integration is
verified on its own.

## One trap worth stating

**A check that enumerates or matches from the same source it validates proves nothing.** A totality
test iterating its own map, a residue check reusing the pattern it verifies, or a drift check
comparing against the recorded sha it is checking, cannot fail for the case it exists to catch.
Verify against an independent oracle: the directory, not the list; the shape, not the pattern; the
live tree, not the pinned baseline.
