# The single-resolution-engine cutover — where it stands, and what it still owes

> **⚠ The landed code contains a cache-poison REGRESSION this work introduced, and it must be fixed
> before this branch is merged onward.** The fallthrough resolver's admission funnel lost its
> non-cacheability rail — not by being edited (the file is byte-identical to its base) but because this
> lineage deleted the ~31 call sites that fed the completeness signal its gate depends on. The rail's
> absence is **proven**; an end-to-end poisoning trace was **never constructed** — so it is a
> proven-missing safety rail, not a demonstrated exploit, and the way to settle it is a **discriminating
> test, not another opinion**. Full mechanism:
> [`cache-admission-closure-design.md`](cache-admission-closure-design.md) §0. **That is job one.**
>
> **Read the rest before you believe anything else is fixed.** What landed is a **checkpoint**, not a
> completed fix. It closes a number of real, individually-proven cache-poisoning holes, but **the poison
> class remains OPEN and REACHABLE in the landed code**, and a reachable stack-overflow crash in the
> shared resolver has **not been started**. Both are specified, implementer-ready, in the companion
> documents. Read this as "the bugs were understood, half-fenced, and written down" — not as "the bugs
> were fixed".

## Status of the landed code — the five facts

Everything below is elaborated later in this document. If you read nothing else, read this table, and
**run the checks in the right-hand column against the tree in front of you** rather than trusting the
middle one — this document was written as the work was landing, and the tree is the authority.

| Item | Status as written | How to check it yourself |
|---|---|---|
| **The shared-cache poison class** | **OPEN and reachable.** Individual sites are closed; the class is not. The known hole set is **not exhaustive**. | Read [`cache-admission-closure-design.md`](cache-admission-closure-design.md). If `install_fact_tracer` still returns a bare `bool` beside the facts, §5 is not done. |
| **The stack-overflow crash** | **NOT FIXED. Not started.** A ~200-deep authored type aborts the process. | `grep -n "fn project_view_node" crates/verter_session/src/project_semantic_dispatch/locator_view.rs` — if it still calls itself, the crash is live. |
| **The fallthrough poison REGRESSION** — introduced by this work; the base did not have it | **UNFIXED. IT SHIPS.** The admission funnel has **no non-cacheability rail at all**. **Fix this first.** | `grep -cE "non_cacheable\|CacheabilityProbe\|with_cacheability_scope" crates/verter_session/src/resolver_core/fallthrough_resolver.rs` ⇒ **0**. See the closure design §0. |
| **The dropped policy seam** (`PolicyContext` never constructed ⇒ workspace-ownership classification lost) | Investigation was **in flight**. **Verify** — if unfixed this is a **live regression** violating a CRITICAL rule. | `grep -rn "PolicyContext {" --include="*.rs" crates/verter_session/src/` — **empty means open.** Restore recipe is below. |
| **Clippy** | Expect **red** (~83 dead-code errors in `verter_session`), **all pre-existing** to this work and mostly the very residue the effort exists to delete. ~13 are design-bearing. | `cargo clippy --workspace -- -D warnings`. Read the landing-hygiene section **before** deleting anything. |

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
**Open — in the landed code, reachable, and specified in the companion documents:**

- **⚠ THE REGRESSION THIS WORK INTRODUCED — and it SHIPS UNFIXED.** One function used to do two things
  at once: mark a request cache-suppressed **and** fold its result to partial. Decoupling those is
  architecturally **correct** (a fenced serve should not make a result *partial*), and this lineage
  deleted all ~31 of its call sites. But `store_node` in
  `crates/verter_session/src/resolver_core/fallthrough_resolver.rs` gated admission on exactly that
  completeness signal — so deleting the fold **rendered its gate toothless**, while its comment still
  claims a "single no-poison rail". The file was never edited (it is byte-identical to its base); the
  rail was removed from underneath it. A fallthrough node computed through a fenced serve or a lease
  miss, carrying non-empty **live-rooted** facts, is admitted and served warm indefinitely — and
  live-rooted facts are exactly the ones the read-side rail can never reject. **The base did not have
  this.** The missing rail is proven by inspection; an end-to-end poisoning trace was never
  constructed, so treat it as a **proven-missing safety rail, not a demonstrated exploit** — and settle
  it with a **discriminating test**, never another static safety argument. →
  **[`cache-admission-closure-design.md`](cache-admission-closure-design.md) §0. This is job one.**

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

### LIVE REGRESSION — the workspace-ownership policy seam was dropped

**This is a regression introduced by the cutover work, and it is the first thing you should check,
because repair was in progress when this was written and may or may not have landed. Check; do not
assume.**

The CRITICAL Typed-IR-Only rule names `ResolverContext::workspace_is_workspace_owned` **by name** as
the required alternative to `node_modules` path-substring checks: symbolic-versus-materialise
decisions must consult workspace ownership structurally, not by sniffing paths. Before the cutover,
the shallow-preserve path did exactly that, in
`crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs`:

```rust
let policy_ctx = crate::component_meta_resolution_policy::policy_helpers::PolicyContext {
    is_workspace_owned: &|canonical| self.ctx.workspace_is_workspace_owned(canonical),
    is_package_backed: &|canonical| self.ctx.workspace_is_package_backed(canonical),
    route_preservation_context: false,
    cycle_active_for_target: false,
    shallow_preserve_list_entry: false,
};
if crate::component_meta_resolution_policy::policy_helpers::imported_ref_must_materialize_canonically(
    &root_identity.canonical_id,
    prepared.as_deref(),
    &policy_ctx,
) { /* … */ }
```

On the cutover lane **that construction is gone**: `PolicyContext` is never constructed anywhere in
`crates/verter_session/src/`, and `workspace_is_workspace_owned` has **no production caller at all** —
only the trait declaration and its impls. The policy seam lost its only entry point, so the
symbolic-versus-materialise decision no longer consults workspace ownership. That is a **live
violation of a CRITICAL rule**, not a stale rule text.

Check it on the tree in front of you:

```bash
grep -rn "PolicyContext {" --include="*.rs" crates/verter_session/src/
grep -rn "workspace_is_workspace_owned" --include="*.rs" crates/verter_session/src/ \
  | grep -v "fn workspace_is_workspace_owned"
```

If the first returns a construction site and the second a real call site, it was repaired before
landing and this item is closed. If either comes back empty it is **open**: restore the seam (the shape
above is the whole of it), or establish deliberately and in writing that the decision genuinely no
longer needs ownership classification — and if the latter, update the CRITICAL rule text that names
the function, because the rule and the code currently disagree.

### A sealed-capability rail may be inert — and one of them provably was

A sealed capability type enforces nothing if it is **never constructed**. On the cutover lane,
`HostManageComponentMetaOutputCap` (declared in
`crates/verter_session/src/host_manage/component_meta_methods.rs`) had **zero construction sites** —
the lane deleted the mint in
`crates/verter_session/src/host_manage/component_meta_methods/macro_output_expansion.rs`, where it was
previously minted as `HostManageComponentMetaOutputCap::new(dispatch)`. A capability that is never
minted is a decorative type, and any guard built on it enforces **nothing**.

Check with `grep -rn "HostManageComponentMetaOutputCap::new(" --include="*.rs" crates/`. If that
returns nothing, either restore the mint or delete the type — do not land a capability rail that is
inert while the documentation claims it is load-bearing. The same smell is worth checking on any other
sealed capability in the tree: **a cap with no construction site is not a guard.**

**One correction to the record, so you do not act on a false lead:** the same "never constructed" claim
was made about `RegistryMemberShapeKeyCap`, and it is **wrong** — that one *is* constructed in
production, in `crates/verter_session/src/meta_resolve/materialize/field_types.rs`. Verify before you
delete.

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
- **`cargo clippy --workspace -- -D warnings` must be green at landing.** Expect it to be red on the
  cutover work, with dead-code errors concentrated in `verter_session` — roughly 83 of them, **present
  unchanged at the work's own base**, so the cutover introduced none of them. They are overwhelmingly
  orphaned `TypeExpr`-era readers whose last callers earlier cutover work deleted: **the dead code is
  precisely the residue this effort exists to delete**, so removing it is on-plan, not a workaround.

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
