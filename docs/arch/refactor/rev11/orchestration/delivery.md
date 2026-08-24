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

## One trap worth stating

**A check that enumerates or matches from the same source it validates proves nothing.** A totality
test iterating its own map, or a residue check reusing the pattern it verifies, cannot fail for the
case it exists to catch. Verify against an independent oracle: the directory, not the list; the
shape, not the pattern.
