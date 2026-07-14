# The single-resolution-engine cutover — where it stands, and what it still owes

> **The cache-poison REGRESSION this work introduced is CLOSED.** An earlier revision of this document
> led with it as shipping unfixed; that is no longer true. The fallthrough resolver's admission funnel
> had lost its non-cacheability rail — not by being edited (the file was byte-identical to its base) but
> because this lineage deleted the ~31 call sites that fed the completeness signal its gate depended on.
> `store_node` now **requires an unforgeable `CacheabilityProbe`**, sampled AFTER the compute it
> encloses, and **refuses the cache write** on a non-cacheable compute while still **serving the value**.
> It was settled the way this document demanded — **by a discriminating test, not an opinion**:
> `fenced_serve_fallthrough_node_is_not_admitted` in
> `crates/verter_session/src/fallthrough_admission_tests.rs` (control + fenced + path-precision arms).
> Full mechanism: [`cache-admission-closure-design.md`](cache-admission-closure-design.md) §0.
>
> **Read the rest before you believe anything else is fixed.** What landed is a **checkpoint**, not a
> completed fix. It closes a number of real, individually-proven cache-poisoning holes — that regression
> among them — but **the poison class remains OPEN and REACHABLE in the landed code**, and a reachable
> stack-overflow crash in the shared resolver has **not been started**. Both are specified,
> implementer-ready, in the companion documents. Read this as "the bugs were understood, several were
> closed, the class was not" — not as "the bugs were fixed".

## Status of the landed code — the five facts

Everything below is elaborated later in this document. If you read nothing else, read this table, and
**run the checks in the right-hand column against the tree in front of you** rather than trusting the
middle one — this document was written as the work was landing, and the tree is the authority.

| Item | Status as written | How to check it yourself |
|---|---|---|
| **The shared-cache poison class** | **OPEN and reachable.** Individual sites are closed; the class is not. The known hole set is **not exhaustive**. | Read [`cache-admission-closure-design.md`](cache-admission-closure-design.md). If `install_fact_tracer` still returns a bare `bool` beside the facts, §5 is not done. |
| **The stack-overflow crash** | **NOT FIXED. Not started.** A ~200-deep authored type aborts the process. | `grep -n "fn project_view_node" crates/verter_session/src/project_semantic_dispatch/locator_view.rs` — if it still calls itself, the crash is live. |
| **The fallthrough poison REGRESSION** — introduced by this work; the base did not have it | **CLOSED.** `store_node` requires an unforgeable `CacheabilityProbe`, samples it after the compute, and refuses the write on a non-cacheable compute while still serving the value. | `grep -cE "non_cacheable\|CacheabilityProbe" crates/verter_session/src/resolver_core/fallthrough_resolver.rs` ⇒ **non-zero**. Run `fenced_serve_fallthrough_node_is_not_admitted`; delete the `probe.non_cacheable()` refusal and it goes **red**. |
| **The workspace-ownership policy seam** | **CLOSED — it was RELOCATED, not lost.** The decision sites call `workspace_is_package_backed` **directly**; the `PolicyContext` seam was genuinely orphaned and is deleted. No classification was lost. | `grep -rln "workspace_is_package_backed" --include="*.rs" crates/verter_session/src/` — the live decision sites are there (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, …). |
| **Clippy** | **RED — 83 errors in `verter_session` (78 `dead_code` + 5 style lints).** Measured on this tree, not inherited from a report. The dead code is the orphaned `TypeExpr`-era residue this effort exists to delete; a cleanup pass was attempted and **reverted** (see below — the naive deletion breaks the TEST build while clippy stays green). **No `#[allow]` was added to hide any of it.** | `cargo clippy --workspace -- -D warnings` ⇒ exit **101**. But **`cargo check -p verter_session --lib --tests` is the command that matters** — it passes, and clippy alone would not have told you that. |

## The goal, unchanged

Verter is supposed to have **exactly one query-time type-resolution engine**:
`SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, in five query modes.
A second engine still exists — the `verter_parser` structural expander under
`crates/verter_parser/src/utils/oxc/script/type_surface/` together with the session-side
`crates/verter_session/src/host_resolve/frontier_engine.rs` — and this effort exists to delete it, by
removing the terminal `TypeExpr` carriers that keep it alive and routing every query-time semantic
decision through the shared dispatch.

That goal is **intact**. An independent architecture reassessment reviewed the remaining path and
found it coherent and minimal for the goal; the detour into the two shared-engine bugs did not distort
it — it hardened the engine that survives.

The finish line matters and is easy to overshoot: **the goal is done when the second engine is
deleted.** The roadmap continuations in the wider plan (the output/display quarantine rename, the perf
compaction) are *continuations*, **not completion conditions**. Do not let them be pulled in.

The design and sequencing skeleton is committed and readable:

- [`../stage10-typeexpr-terminal-removal-design.md`](../stage10-typeexpr-terminal-removal-design.md)
  — the binding design: what a terminal carrier is, the four-source locator/fact model, the scope
  boundary, and the rule that landed guards are structural (compiler, privacy, sealing) and never
  name-keyed source scanners.
- [`../stage10-b3-b7-execution-plan.md`](../stage10-b3-b7-execution-plan.md) — the sequencing
  authority and the file-touch sets.
- [`../semantic-db-overhaul-unified-remaining-plan.md`](../semantic-db-overhaul-unified-remaining-plan.md)
  — the wider programme this sits inside.

## A note on what you can and cannot see

The work described here ran across several local-only branches and a large volume of scratch material
— design briefs, consult transcripts, review dossiers, a working ledger. **All of it was machine-local
and none of it was pushed. It no longer exists.** Do not go looking for it; nothing in these documents
depends on it. Everything of substance was carried into this directory precisely because those
artefacts were known to be doomed.

One consequence worth stating plainly: an **independent second implementation** of the cache-admission
fix was written, and it solved one part of the problem better than the version that landed. **That
implementation is gone.** Its superior design is not — it is specified in full in
[`cache-admission-closure-design.md`](cache-admission-closure-design.md) §5, written so it can be
rebuilt from the prose without ever seeing the diff. **Rebuild it; do not hunt for it.**

## What the checkpoint DOES close, and what it leaves open

**Closed — each proven with a test that goes red against the pre-fix tree:**

- A completeness rail that had been **erasing its own signal**: partial results now carry typed
  reasons, ride a node-hiding outcome type, and fold centrally so a partial taints its build frame and
  **refuses warm admission** instead of laundering into caches as `Complete`.
- An unforgeable cacheability probe — private field, single constructor, an HRTB preventing it from
  escaping its scope — now **required** by several shared-cache funnels, so an untraced producer at
  those funnels is a **compile error** rather than a review miss. Two independent adversarial reviews
  confirmed it is genuinely unforgeable.
- A set of individually-proven live poison sites: a member-shape cache admitting entries derived from
  unpublished serves (reachable on `defineProps<{ msg: MyStr }>()` — the most ordinary shape in the
  repo); an owner-collection cache admitting a **degraded `None`** on a lease miss, rooted on the
  **live** hash so it permanently shadowed a recoverable declaration; two further live caches that **no
  brief had named** and that had no tracer at all; a temporal hole where the probe was sampled at
  funnel entry while the compute ran later; and a singleflight follower that could adopt an unadmitted
  result.
- **The fallthrough regression this work itself introduced.** One function used to do two things at
  once: mark a request cache-suppressed **and** fold its result to partial. Decoupling those is
  architecturally **correct** (a fenced serve should not make a result *partial*), and this lineage
  deleted all ~31 of its call sites — but `store_node` in
  `crates/verter_session/src/resolver_core/fallthrough_resolver.rs` gated admission on exactly that
  completeness signal, so deleting the fold **rendered its gate toothless**. The file had never been
  edited; the rail was removed from underneath it. It is now closed at the funnel itself, which is the
  durable place: `store_node` **requires** an unforgeable `CacheabilityProbe` — private field, single
  constructor, an HRTB preventing escape — whose scope **encloses** the compute and which is sampled
  **after** it, and it **refuses the cache write** on `probe.non_cacheable()` while still **serving the
  value** to the caller. An untraced producer is now a **compile error** at that funnel, not a review
  miss. Proven by `fenced_serve_fallthrough_node_is_not_admitted`
  (`crates/verter_session/src/fallthrough_admission_tests.rs`): a **control** arm proves an ordinary
  compute admits (so the refusal assertion cannot pass vacuously on an absent compute), a **fenced** arm
  proves refusal-while-served, and a **path-precision** arm proves the fence does not blanket-refuse.

**Open — in the landed code, reachable, and specified in the companion documents:**

- **The poison class is not closed.** The probe proves a tracer was *active at admission*; it does
  **not** prove the *compute ran inside it*. A caller can compute first and then open an empty scope to
  obtain a probe. Several funnels take no probe at all, and raw cache mutators remain reachable from
  production-visible types — some `#[doc(hidden)]` with **no cfg gate**, shipping in release. Worse,
  **the known hole set is not exhaustive**: the architecture consult found two live exposures nobody
  had asked it about, and stated it cannot prove exhaustiveness while mutation remains decentralised.
  → **[`cache-admission-closure-design.md`](cache-admission-closure-design.md)**
- **A reachable stack-overflow crash is not started.** The shared projection primitive recurses on the
  host stack per level of structural nesting, per demand; a ~200-deep authored type aborts the process
  before any fuse trips, and it is reachable by **any** dispatch consumer. →
  **[`shared-engine-crash-fix-design.md`](shared-engine-crash-fix-design.md)**

## The remaining sequence

The order is not arbitrary — each step's safety argument depends on the one before it.

1. **Close the cache-admission substrate.** Invert scope ownership so the cache **owner** opens the
   tracing scope, runs the cold closure inside it, and mints a sealed by-value token that the raw write
   is the sole consumer of. **Make non-cacheability intrinsic to the finalise type** (§5 of the closure
   design — it kills the droppable boolean that is the root-cause shape of the whole class). Delete the
   zero-caller APIs, compile-confine every raw mutator behind the existing `test-support` feature, and
   then **audit — do not patch**: enumerate every shared-cache producer and prove each one either takes
   the capability or structurally cannot admit. Patching sites has already failed three times.
2. **Fix the crash.** Rewrite the shared projection primitive as an explicit heap worklist, add the
   dual-rail fuse (work ceiling + cross-query host-recursion depth; **no structural-depth cap**), and
   pin it with crash regressions that run in a **2 MB-stack subprocess** — the workspace stack setting
   **hides** the crash.
3. **Reintroduce the reducer** against the *new* primitive. An earlier work-in-progress version was
   built against the recursive primitive and carried a stack-safety claim that is empirically **false**;
   it was discarded and no longer exists. Rebuild it against the new primitive, where its only
   mechanical hazard is adapting to the `Complete` / `Partial` outcome.
4. **Do the production cutover** — route the remaining production surfaces onto the shared dispatch.
5. **Delete the second engine's expander** — reduce the parser `type_surface` module to syntax-only and
   land the terminal guards.
6. **Move the engine physically** — extract query-time semantic execution into a `verter_session_query`
   package whose dependency closure excludes `verter_parser`, `verter_compiler`, `verter_type_expr_oxc`
   and `oxc_*` under all features, enforced by a dependency-graph firewall. The package does not exist
   yet. This is what makes the single-engine rule **structurally** true rather than merely observed.
7. **Squash and land.** The goal is done at this point.

## Open defects that are not the two bugs

### RESOLVED — the workspace-ownership policy seam was RELOCATED, not lost

**An earlier revision of this document carried this as a possible live regression. It is not one.** It
was settled the way this record demands — **by test, not by argument** — and the finding is recorded
here so nobody re-opens it or "restores" a seam that should stay deleted.

The concern was that the cutover had dropped workspace-ownership classification. The evidence looked
damning: `PolicyContext` — the struct that used to carry the ownership predicates into the
shallow-preserve path — is **never constructed anywhere** in `crates/verter_session/src/`.

That evidence was real but the conclusion drawn from it was wrong. **The classification did not go
away; its delivery mechanism did.** The live decision sites now call the ownership predicate
**directly** rather than routing it through a struct of closures:

```bash
grep -rln "workspace_is_package_backed" --include="*.rs" crates/verter_session/src/
```

That returns the real decision sites — among them `component_meta_materialize.rs`,
`framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`,
`meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`,
`project_semantic_dispatch/raise.rs` and `walk.rs`. Workspace ownership is still consulted
structurally, exactly as the CRITICAL rule requires; `PolicyContext` was a genuinely orphaned
indirection and is deleted. **Do not restore it.**

One durable correction falls out of this, and it is the reason the investigation went the way it did:
the CRITICAL Typed-IR-Only rule used to name `ResolverContext::workspace_is_workspace_owned` **by
name** as the required alternative to `node_modules` substring checks — **and that function no longer
exists on the tree.** The rule text now names `workspace_is_package_backed`, which is what actually
survives (workspace-owned is its complement). A CRITICAL rule naming a nonexistent function is exactly
how the next reader concludes a seam was lost when it was only moved.

### THE BIGGEST OPEN CHORE: clippy is red, and the obvious way to fix it does NOT work

**Read this before you try to make clippy green.** Somebody already tried the obvious thing, it failed,
and the failure is instructive. The tree you have **compiles — lib and tests both**. What it does not do
is pass `cargo clippy --workspace -- -D warnings`, because it still carries the orphaned `TypeExpr`-era
residue as `dead_code`.

**The trap, stated up front.** `cargo clippy --workspace` checks the **lib** target only. The **test**
target is a separate compilation. So you can delete a "dead" item, watch clippy go green, and have
**broken the test build without ever being told**. The command that tells you the truth is:

```bash
cargo check -p verter_session --lib --tests     # ← the floor. Not clippy.
```

Deleting the residue naively produces exactly this, and it is what defeated the attempt:

```
error[E0425]: cannot find function `type_expr_materialize_reduction_context` in module `crate::meta_resolve::materialize`
note: function `...::field_types::type_expr_materialize_reduction_context` exists but is inaccessible
error[E0599]: no method named `eq_to_expr` found for struct `NodeShapeEq`
error[E0599]: no method named `materialize_registry_routed_member_surface` found for struct `ComponentMetaQueryEngine`
```

**Root cause, and it is worth understanding rather than just patching.** Two independent efforts fed
into this checkpoint:

1. the cache-admission fix work (the probe rail, the poison-site closures), and
2. a dead-code cleanup that deleted the orphaned `TypeExpr`-era residue.

The cleanup computed its dead-code census **against an older base than the fix work finished on**. The
fix work then **added tests** that exercise parts of that supposedly-dead `TypeExpr`-era cluster. So the
census went **stale**: items the cleanup had correctly proven dead *at its base* had, by the time both
were combined, acquired **test callers**. Deleting them therefore breaks the test build — while the
**lib** build stays perfectly green, because the callers are all `#[cfg(test)]`.

That asymmetry is the trap, and it generalises: **"dead" is relative to a target and to a base.** An
item with no production caller but a live test caller is dead to the lib and alive to the tests.
`cargo clippy --workspace` sees only the first.

**What this checkpoint actually did — and you need to know it, because it is a deliberate NON-landing.**
The dead-code cleanup was attempted on top of the fix work and then **reverted**. The source tree you
have is the **fix work, unmodified**; the cleanup's deletions are **not in it**. That is why clippy is
red: the residue the cleanup would have removed is all still here.

That was not laziness, and the reasoning is the useful part. The integration was tried, and it failed in
a way that kept getting worse the further it went. Each deletion the cleanup made, when combined with the
fix work, broke something the fix work's tests used — and the breakages surfaced **one compile at a
time**, because the lib kept building green while the *test* target broke. The collision set grew with
every round (`field_types` helpers → the `raise`/`shape_engine` predicates →
`MaterializedOutputTypeExpr::into_type_expr` → `NodeShapeEq::eq_to_expr` → the `materialize`
re-exports → `utility_types` → the `component_meta_query_engine` surface/helpers cluster). At that point
the honest conclusion is that **the cleanup's census and the fix work's test surface are not reconcilable
by patching** — they need one deliberate pass, not a merge.

So the checkpoint chose a tree that is **known-good and verifiable** over one that is half-merged and
unverifiable. Nothing was silenced with `#[allow]`, and **no test was deleted to make a build pass** —
deleting a test to satisfy a compiler is how coverage quietly dies.

**The correct closing move — this is the work, and it is a real piece of work.** These items are
genuinely residue: they have **no production caller**, and the tests that pin them are testing a
`TypeExpr`-era path this whole effort exists to delete. Retire them as ONE change, not as a deletion
followed by a repair:

1. **Start from the test target, not clippy.** `cargo check -p verter_session --lib --tests` is the
   floor. `cargo clippy --workspace` checks the **lib only** and will tell you everything is fine while
   the test build is broken — that is precisely the trap that defeated the merge.
2. For each `dead_code` item, find its test callers (`query_db_self_root_tests.rs`,
   `field_types_tests.rs`, the shape-cache tests) and ask the only question that matters: **is this test
   the discriminating coverage for something that still exists?** If it only pins the dead `TypeExpr`
   path, **delete the item and its test together, in the same commit.** If it pins live behaviour, the
   item is not dead — it is mis-placed, and belongs behind a `#[cfg(test)]` compile-gate (honest) rather
   than an `#[allow]` (not).
3. Do **not** blind-delete either half. The cluster contains at least one item —
   `RegistryMemberShapeKeyCap` — about which a "never constructed" claim was already made **and was
   wrong**: it is minted in production in `field_types.rs`. Acting on that claim unverified would have
   deleted a live mint.

**Never resolve this by deleting a live caller, and never by `#[allow(dead_code)]`.** The blanket allow
would neuter the shrinking ledger that tracks exactly this residue — the one instrument that tells you
whether the cutover is converging.

### A sealed-capability rail may be inert — keep checking, one provably was

A sealed capability type enforces nothing if it is **never constructed**. A capability that is never
minted is a decorative type, and any guard built on it enforces **nothing**. This is a live smell in
this tree, worth re-checking on **any** sealed capability you meet: **a cap with no construction site
is not a guard.**

The specific instance that prompted this — `HostManageComponentMetaOutputCap`, which had zero
construction sites — was **resolved by deleting the type**, which is the right outcome for an inert
rail: it is gone from `crates/verter_session/src/`, surviving only as a name in a residual guard's
list. Do not go looking for it, and do not restore it.

**One correction to the record, so you do not act on a false lead:** the same "never constructed" claim
was made about `RegistryMemberShapeKeyCap`, and it is **wrong** — that one *is* constructed in
production, in `crates/verter_session/src/meta_resolve/materialize/field_types.rs`. It is load-bearing.
**Verify before you delete** — that claim, acted on unverified, would have removed a live mint.

### An unverified claim — do not repeat it as fact

A reviewer asserted that the request-sticky mechanism deleted during the cache work was an `Arc`
crossing scheduler-worker threads, whereas the thread-local tracer that replaced it is not — implying a
lost cross-thread signal. **Nobody proved or disproved it**, and if true it would apply equally to
*both* independent implementations, so it was never a differentiator. Settle it with a test that drives
a demand across a scheduler worker boundary and asserts the non-cacheability mark survives. Do not
carry it forward as a finding.

## Landing hygiene

- **The cutover must land as a SQUASH.** Its development history contained commits that **do not
  compile**, so `git bisect` over it is unsound and any "commit X was green" reasoning over it is
  unsafe. *(Reported, not re-verifiable — the history in question was local-only.)*
- Rebase onto the current branch and run the **full** workspace gate (`node scripts/gate.mjs`) **before**
  the squash, not after.
- Scrub planning vocabulary from source comments and commit messages: the code reads as final state.
- **`cargo clippy --workspace -- -D warnings` is RED at this checkpoint: 83 errors, exit 101** — 78
  `dead_code` plus 5 style lints, all in `verter_session`. Measured on this tree. The library and the
  tests both **compile**; not one of the 83 is a type error. **No `#[allow]` was added**; nothing is
  hidden.

  The dead code is the orphaned `TypeExpr`-era cluster — `materialize_component_meta_type_expr_until_stable(_full)`,
  `stabilize_registry_member_surface_node_with_shape_cache`, `lower_type_expr_for_shape_subject`,
  `type_expr_materialize_reduction_context`, `RegistryMemberShapeKeyCap`, the `node_root_is_typeof`
  chain, `MaterializedOutputTypeExpr::into_type_expr`, the `synthetic_carrier_guard` walkers — i.e.
  **precisely the residue this effort exists to delete.** Removing it is on-plan, not a workaround.

  **But do not just delete it.** A cleanup that did exactly that was attempted on top of this work and
  had to be reverted: those items have no production caller but they DO have `#[cfg(test)]` callers, so
  deleting them breaks the **test** build while `cargo clippy --workspace` — which only checks the
  **lib** — stays green and tells you nothing. The full account, and the way to actually close it, is in
  the section above ("THE BIGGEST OPEN CHORE"). Read it before you touch a single `dead_code` item.

  Two cautions. First, **hidden debt**: because the `verter_session` library aborts under `-D warnings`,
  the crates downstream of it (`verter_ffi`, `verter_lsp`, `verter_mcp*`, `verter_wasm`, `verter_napi`)
  are **never clippy-checked** while that wall stands — more may sit behind it. Second, roughly
  **thirteen of those items are design-bearing and must not be blind-deleted**: the two defects above
  are among them, and a third — `ProjectSemanticDispatch::replay_session_demand_to_hot`, in
  `crates/verter_session/src/project_semantic_dispatch/semantic_source.rs` — is a **sanctioned**
  deferral, because `CLAUDE.md` explicitly describes the hot-prepared carriers as dead-code-correct
  scaffolding whose production wiring is deferred. Deleting it would contradict a stated architectural
  decision.

  **No blanket `#[allow(dead_code)]`** — it would neuter the shrinking ledger that tracks exactly this
  residue.
