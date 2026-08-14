# Optimization vs. structural conformance — investigation

Codex xhigh, sandbox read-only, dispatched 2026-08-13 to investigate a maintainer suspicion
following the mapping-oracle correction (see `mapping-oracle-scoping-consult.md` in this
directory): does BF2's structural comparator force Verter's codegen to replicate the official
Vue/Svelte compiler's internal structural/topology choices even where a different,
Verter-optimized structure would be behaviorally equivalent?

Maintainer framing (verbatim, across two messages): "verter only golden check is to make sure
verter outputs a similar and valid js output that does the same at runtime, cosmetic differences
are allowed, since the content is not byte perfect sourcemap cannot be the same obviously" / "i
only care about the goldens for building, i want verter to be optimised, i suspect the official
vue and svelte compiler might generate content in a way that verter replicates it wont be
optimised for verter architecture" / (after this investigation was dispatched) "if the
optimisation is too much we can match official output, just bear in mind the other things ive
said" — i.e. default to matching official structurally; the exception is invoked only where
matching has a real, demonstrated optimization cost, not as a general policy loosening.

## Verdict

**PARTIALLY CONFIRMED.** The architectural overconstraint is real, and recent history contains
concrete behavior-neutral official-shape matching. One instance adds objectively unnecessary
emitted code. However, no evidence was found that Verter has yet been forced into a worse core
reactivity/effect/memoization/DOM algorithm merely because Vue or Svelte chose it.

- **Confirmed:** BF2's current JavaScript comparator is substantially stricter than "behavior plus
  behaviorally meaningful topology" — it is effectively position-free ESTree identity comparison
  (`normalize.mjs`/`compare.mjs`), not semantic-topology equivalence. It has no logic corresponding
  to CLAUDE.md's own "where order is semantic" qualifier — every order is currently structural.
- **Confirmed concrete waste:** commit `ec6efe184` changed VDOM `v-if` handling so a fully covered
  `v-if`/`v-else` chain still imports `createCommentVNode` even though the body never calls it,
  solely to match the official golden's imported-but-unused helper artifact. Extra generated
  bytes, an extra named binding, no runtime use — the clearest confirmed instance of Verter
  reproducing an official implementation artifact its own architecture did not need.
- **Confirmed arbitrary-shape enforcement, no measured cost:** two cells failed solely on
  `_sfc_main`/template-helper-import module-item order; ESM imports are hoisted so both orders
  execute identically. History shows real churn (`ec6efe184` → `a9ca78e3c` → `a62dec322`)
  fighting over which arbitrary order to match, with no evidence either order is faster.
- **Confirmed comparator defect, no production cost yet:** named import-specifier order
  (`import { a, b }` vs `{ b, a }`) was treated as structurally different with no semantic basis.
  Track B's own BF2 reopen #4 work (`work/bf2-reopen4`, commit `7abc4c127`) independently found and
  fixed this before this investigation completed — sorts named specifiers only, preserves
  membership/aliases/source/side-effect-import order.
- **Not found:** a materially worse Verter-native reactivity, memoization, hydration, or DOM
  architecture being rejected in favor of an arbitrary official one. Every other structural fix
  examined this program (the `__vapor` marker, the Vapor insertion anchor, Vapor helper/event
  routing, `__expose`, SSR binding routing, fragment/key topology, slot-fallback caching) was
  behaviorally load-bearing, not an arbitrary official shape imposed on an equally-valid Verter
  alternative.
- **Svelte:** no Revision 11 Svelte production conformance train has run yet (BS1 not started), so
  there is no comparable Svelte fix history to classify.

## Disposition

Two distinct scopes, per "Explicit finding disposition" (CLAUDE.md):

1. **Narrow comparator corrections (named-specifier order, unused-helper-import waste when
   provably unreachable and link-safe): `ADOPT-NOW`.** These are within BF2's existing normalizer
   authority (BF2 already distinguishes cosmetic normalization from semantic structure) — no new
   ratification needed. Named-specifier order is already landed on `work/bf2-reopen4`. The
   unused-import case should be evaluated for the same treatment in the same reopen if it can be
   done narrowly and safely (binding provably unreachable, source module load-equivalent, exact
   link behavior preserved) — track orchestrator's call within existing authority.

2. **General comparator redesign (normalized-AST-identity → behaviorally-load-bearing-topology,
   the proposed three-layer model of independent hard oracles + semantic topology witnesses +
   adversarial differential execution): `DEFER`.** This changes CLAUDE.md's own standing
   cross-program CRITICAL "Compiled-Output Conformance" rule and AMD-005's ratified
   acceptance/normalizer contract — the highest ratification bar in this program, not something a
   track orchestrator or this consult can adopt unilaterally. Per the maintainer's own
   "if the optimisation is too much we can match official output" framing, this is explicitly not
   urgent: the default (match official structurally) stays in force except where a concrete,
   demonstrated optimization cost is found, handled case-by-case as an `ADOPT-NOW` narrow
   correction (scope 1 above) rather than a wholesale policy change.

**Durable owner:** whichever block next does a full production-conformance push at scale where
this tension would recur systematically — most likely BV1 (Vue) or BS1 (Svelte), matching the
existing debt-ownership pattern (see `debt-BF2-perf-gate-deferred.md` in this directory for the
precedent). Until then, individual narrow overconstraints found during BF2/BV0/BF3 work are fixed
case-by-case under scope 1 as they're found — this is not a blocking gate on current work.

**Acceptance ID:** `FC-VUE-002` — "the conformance comparator's structural axis distinguishes
behaviorally load-bearing topology from official's arbitrary internal implementation choice,
via independent hard oracles + semantic topology witnesses + adversarial differential execution,
rather than normalized-AST identity." Not required for BF2/BV0/BF3's current exits. Owned by
whichever block first needs it (BV1/BS1, most likely).

## Full investigation text

See the Codex session transcript preserved at time of writing in the program orchestrator's
scratchpad if needed for the complete evidence trail (concrete file/line citations for every
finding above); the summary in this document is the authoritative disposition record.
