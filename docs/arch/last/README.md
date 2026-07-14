# `docs/arch/last/` — the consolidated handoff record

## READ THIS FIRST: what is closed, and what is still open

**The fallthrough cache-poison regression this work introduced is CLOSED.** An earlier revision of
this record led with it as shipping unfixed; that is no longer true, and the correction is the first
thing to know so you do not go hunting for a bug that is gone.

What it was: the fallthrough resolver's admission funnel — `store_node` in
`crates/verter_session/src/resolver_core/fallthrough_resolver.rs` — had **no non-cacheability rail at
all**. The file itself had never been edited; what changed was underneath it. The lineage deleted the
~31 call sites that folded non-cacheability into cold-compute completeness, and **that completeness
signal was `store_node`'s only safety gate.** Removing the fold left the gate toothless, so a
fallthrough node computed through a fenced serve or a lease miss — carrying non-empty **live-rooted**
facts — was admitted and served warm indefinitely. Live-rooted is the sting: the facts revalidate
against the live view on every warm hit, so the read-side rail can never evict it.

How it is closed: `store_node` now **requires an unforgeable `CacheabilityProbe`** (private field,
single constructor, an HRTB that stops it escaping its scope) whose tracer scope **encloses** the
compute, samples it **after** the compute runs, and **refuses the cache write** when
`probe.non_cacheable()` — while still **serving the value** to the caller. Cache non-admission is not
a failed request. An untraced producer cannot reach the funnel at all: it is a compile error, not a
review miss.

The regression was settled the way this record demanded — **by a discriminating test, not an
opinion**: `fenced_serve_fallthrough_node_is_not_admitted` in
`crates/verter_session/src/fallthrough_admission_tests.rs`. It has three arms, and the shape matters
if you ever touch this rail: a **control** arm proving an ordinary compute *does* admit (so a
zero-candidate assertion under the fence is a refusal, not an absent compute), a **fenced** arm
asserting refusal while the caller is still served, and a **path-precision** arm proving the fence
does not blanket-refuse. Delete the `probe.non_cacheable()` refusal from `store_node` and that test
goes red.

**This does not close the class.** See below — the poison class it belonged to is still open and
reachable, and the known hole set is not exhaustive. One instance being closed is not the invariant
being established.

Full mechanism, and the design for the class:
**[`cache-admission-closure-design.md`](cache-admission-closure-design.md) §0.**

---

> Beyond that regression: **the cache-poisoning class it belongs to is OPEN and REACHABLE in the landed
> code**, and a **reachable stack-overflow crash** in the shared resolver has **NOT been started**. What
> landed is a **checkpoint** — it closes several individually-proven poison sites, but it does not close
> the class. These documents are what you **execute from**, not a history of what happened.

## This is all that survives — by design

The effort that produced these documents ran on one machine, with a working ledger in a gitignored
directory, dozens of design briefs and consult transcripts in temporary scratch directories, and
several long-lived local branches. **None of that exists any more, and none of it was pushed.** If
something is not in this repository, it is gone.

That is why these files carry **content, not pointers**. Every design decision, every mechanism and
every piece of evidence that mattered is written out here in full, so it can be implemented by
someone who has never seen any of the original artefacts. **You should find no reference in these
documents to a file, branch or commit you cannot resolve** — if you do, treat it as a bug in the
document, not as something to go hunting for.

## Read in this order

1. **[`single-engine-cutover-state.md`](single-engine-cutover-state.md)** — the goal, what actually
   landed versus what is merely written, the remaining sequence, the open defects, and landing
   hygiene. **Start here.**
2. **[`cache-admission-closure-design.md`](cache-admission-closure-design.md)** — the headline
   remaining deliverable, implementer-ready: the invariant, why the class kept regrowing (the
   root-cause account everyone worked from was **false**), the ruled mechanism (invert scope
   ownership), a full specification of the type change that kills the root cause — written so you can
   build it from the prose alone — the four known live holes, and the mandate to **audit rather than
   patch**, because the known hole set is **not exhaustive**.
3. **[`shared-engine-crash-fix-design.md`](shared-engine-crash-fix-design.md)** — the reachable
   stack-overflow crash, implementer-ready: the iterative heap-worklist rewrite of the shared
   projection primitive, the dual-rail fuse, and the crash regressions that **must** run in a 2 MB
   subprocess because the workspace stack setting **hides** the crash.
4. **[`verification-traps.md`](verification-traps.md)** — four ways this toolchain hands you a **false
   green**, and the two reasoning failures that let a proven bug hide for three review rounds. Read it
   before you trust any "the gate is clean" claim, including your own.

## Conventions

Claims carry their evidence. Anything verified first-hand against the committed tree cites the file
and symbol, and **you can check it**. Anything asserted from work that no longer exists is labelled
**(reported, not re-verifiable)** — re-derive it if it is load-bearing for you. Where two sources
contradicted each other, the contradiction is written down rather than silently resolved. A question
that could not be settled appears as an open question with a named way to settle it — never as a fact.

Line numbers drift. **Paths and symbol names are the durable part of any citation**; treat a line
number as a hint, not an address.

## Getting started on a fresh machine

```bash
pnpm install                                  # required before any JS/TS test or workspace Node script
node scripts/gate.mjs                         # THE canonical Rust gate — builds once, runs BOTH surfaces
cargo check -p verter_session --lib --tests   # PASSES. This — not clippy — is the compile floor.
cargo clippy --workspace -- -D warnings       # RED: 83 errors (78 dead_code + 5 style), exit 101.
                                             # Expected. Read the state doc BEFORE deleting any of
                                             # them: the naive deletion breaks the TEST build while
                                             # clippy (lib-only) stays green. That trap already cost
                                             # one reverted attempt.
cargo fmt --all --check
pnpm test                                     # only if you touched TypeScript
```

Four things to know before you trust any result:

- **`node scripts/gate.mjs` is the gate.** A bare `cargo test --workspace --tests` **silently skips
  roughly 4,400 tests** (feature unification drops the `verter_session` integration binaries) and
  exits 0 looking healthy. Never use it as a gate.
- **A shell pipeline lies about exit status.** `cargo clippy … | tee out.txt` returns **tee's** status,
  not clippy's — it always "passes". Every false "clippy is clean" report in this effort was this.
- **Crash regressions need a 2 MB stack pinned inside a subprocess**, or the bug they exist to catch
  **hides** and they pass vacuously. See
  [`shared-engine-crash-fix-design.md`](shared-engine-crash-fix-design.md) §6.
- **A passing suite is not proof.** Before claiming a fix is covered, revert your own change and
  confirm the test goes **red**. A zero-coverage "fix" was caught in this codebase by exactly that
  check, after review had passed it.

All four, plus the rest, are in [`verification-traps.md`](verification-traps.md). Read it **before**
you report anything as green.
