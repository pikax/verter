# The single-resolution-engine cutover — where it stands, and what it still owes

> **Read this before you believe anything is fixed.** What lands here is a **checkpoint**, not a
> completed fix. It closes a number of real, individually-proven cache-poisoning holes and it repairs
> a regression the work itself introduced. **The poison class it was fighting remains OPEN and
> REACHABLE in the landed code**, and a reachable stack-overflow crash in the shared resolver has
> **not been started**. Both are specified, implementer-ready, in the companion documents. Do not
> read this as "the bugs were fixed"; read it as "the bugs were understood, half-fenced, and written
> down."

## The goal, unchanged

Verter is supposed to have **exactly one query-time type-resolution engine**:
`SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, in five query modes.
A second engine still exists — the `verter_parser` structural expander under
`crates/verter_parser/src/utils/oxc/script/type_surface/` together with the session-side
`crates/verter_session/src/host_resolve/frontier_engine.rs` — and this effort exists to delete it, by
removing the terminal `TypeExpr` carriers that keep it alive and routing every query-time semantic
decision through the shared dispatch.

That goal is **intact**. An independent architecture reassessment (two code-grounded legs, converged)
reviewed the remaining path and found it coherent and minimal for the goal; the detour into the two
shared-engine bugs did not distort it — it hardened the engine that survives.

The finish line matters and is easy to overshoot: **the goal is done when the second engine is
deleted.** The roadmap continuations in the wider plan (the output/display quarantine rename, the
perf compaction) are *continuations*, **not completion conditions**. Do not let them be pulled in.

The design and sequencing skeleton lives in:

- [`../stage10-typeexpr-terminal-removal-design.md`](../stage10-typeexpr-terminal-removal-design.md)
  — the binding design: what a terminal carrier is, the four-source locator/fact model, the scope
  boundary, and the rule that landed guards are structural (compiler, privacy, sealing) and never
  name-keyed source scanners.
- [`../stage10-b3-b7-execution-plan.md`](../stage10-b3-b7-execution-plan.md) — the sequencing
  authority and the file-touch sets.
- [`../semantic-db-overhaul-unified-remaining-plan.md`](../semantic-db-overhaul-unified-remaining-plan.md)
  — the wider programme this sits inside.

## Where the code actually is

**None of the cutover implementation is on the landing branch.** Verified against the tree at the
time of writing: `refactor/semantic-db-overhaul` carries the design documents and a build-cache
helper; the second engine is present in full; the `verter_session_query` package the design calls for
does not exist; and none of the cache-admission machinery exists on it. Landing is **atomic by
design** — the whole cutover squashes into one commit — so "nothing has landed" is the expected
state, not a regression. (The checkpoint described in this document was in flight as this was
written; `git log` is the authority, not this sentence.)

| Branch | Tip at time of writing | What it is |
|---|---|---|
| `refactor/semantic-db-overhaul` | `1a74626e1` | The landing target. Design docs + `scripts/sccache-env.mjs`. |
| `stage10/b6` | `9aedb2078` | The implementation lane: 372 commits ahead of the merge-base `09cca5a71`; 509 files, +82k/−54k. Never pushed. |
| `codex/bugb-independent` | `4cc13cfbb` | An **independent** second solve of the cache-admission bug, from base `44d2a7528`. 101 files, +8544/−2998. **Preserve it** — it is the port source for the stronger finalise type. |
| `mom/gate-green` | `a697b7be7` | Six commits over `44d2a7528`: deletion of dead `TypeExpr`-era readers, five style lints, and `scratchpad/` finally added to `.gitignore`. |

## What the checkpoint DOES close, and what it leaves open

**Closed — and each was proven with a test that goes red against the pre-fix tree:**

- A completeness rail that had been **erasing its own signal**: partial results now carry typed
  reasons, ride a node-hiding outcome type, and fold centrally so that a partial taints its build
  frame and **refuses warm admission** instead of laundering into caches as `Complete`.
- An unforgeable `CacheabilityProbe` — private field, single constructor, HRTB preventing escape —
  now **required** by several shared-cache funnels, so an untraced producer at those funnels is a
  **compile error** rather than a review miss. Both review legs confirmed it is genuinely
  unforgeable.
- A set of individually-proven live poison sites: a member-shape cache admitting entries derived from
  unpublished serves (reachable on `defineProps<{ msg: MyStr }>()` — the most ordinary shape in the
  repo); an owner-collection cache admitting a **degraded `None`** on a lease miss, rooted on the
  **live** hash so that it permanently shadowed a recoverable declaration; two further live caches
  (`ImportedRegistryDb`, `DeclarationLookupDb`) that **no brief had named** and that had no tracer at
  all; a temporal hole where the probe was sampled at funnel entry while the compute ran later; and a
  singleflight follower that could adopt an unadmitted result.
- **The regression this work introduced itself.** A function that used to do two things at once —
  mark a request cache-suppressed **and** fold its result to partial — was correctly decoupled (a
  fenced serve should not make a result *partial*), but the replacement marked only the thread-local
  tracers, and **no fallthrough file takes a probe**. The fallthrough cache's admission gate, which
  reads only "is the cold compute partial?", therefore started seeing `Complete` and **admitting the
  poison**, with a comment still describing a rail that no longer existed. Both independent
  implementations were **byte-identical** here — a shared blind spot, not a differentiator.

**Open — in the landed code, reachable, and specified in the companion documents:**

- **The poison class is not closed.** The probe proves a tracer was *active at admission*; it does
  **not** prove the *compute ran inside it*. A caller can compute first and then open an empty scope
  to obtain a probe. Several funnels take no probe at all, and raw cache mutators are still reachable
  from production-visible types — some of them `#[doc(hidden)]` with **no cfg gate**, shipping in
  release. Worse, **the known hole set is not exhaustive**: the architecture consult found two live
  exposures nobody had asked it about and said plainly it cannot prove exhaustiveness while mutation
  remains decentralised. → **[`cache-admission-closure-design.md`](cache-admission-closure-design.md)**
- **A reachable stack-overflow crash is not started.** The shared projection primitive recurses on
  the host stack per level of structural nesting, per demand; a ~200-deep authored type aborts the
  process before any fuse trips, and it is reachable by **any** dispatch consumer. →
  **[`shared-engine-crash-fix-design.md`](shared-engine-crash-fix-design.md)**

## The remaining sequence

The order is not arbitrary — each step's safety argument depends on the one before it.

1. **Close the cache-admission substrate.** Invert scope ownership so the cache **owner** opens the
   tracing scope, runs the cold closure inside it, and mints a sealed by-value token that the raw
   write is the sole consumer of. Delete the zero-caller APIs. Compile-confine every raw mutator
   behind the existing `test-support` feature. **Port the three-variant `FactReadSetFinalise` from
   `codex/bugb-independent` @ `4cc13cfbb`**, which makes non-cacheability intrinsic to the type
   instead of a droppable boolean beside it. Then **audit — do not patch**: enumerate every
   shared-cache producer and prove each either takes the capability or structurally cannot admit.
   Patching sites has failed three times.
2. **Fix the crash.** Rewrite the shared projection primitive as an explicit heap worklist, add the
   dual-rail fuse (work ceiling + cross-query host-recursion depth; **no structural-depth cap**), and
   pin it with crash regressions that run in a **2 MB-stack subprocess** — the workspace
   `RUST_MIN_STACK=128MB` **hides** the crash.
3. **Reintroduce the reducer** against the *new* primitive. The earlier work-in-progress version was
   built against the recursive primitive and carried a stack-safety claim that is empirically false;
   it was discarded and is reflog-preserved only. **Do not resurrect it.**
4. **Do the production cutover** — route the remaining production surfaces onto the shared dispatch.
5. **Delete the second engine's expander** — reduce the parser `type_surface` module to syntax-only
   and land the terminal guards.
6. **Move the engine physically** — extract query-time semantic execution into a `verter_session_query`
   package whose dependency closure excludes `verter_parser`, `verter_compiler`,
   `verter_type_expr_oxc` and `oxc_*` under all features, enforced by a dependency-graph firewall.
   The package does not exist yet. This is what makes the single-engine rule **structurally** true
   rather than merely observed.
7. **Squash and land.** The goal is done at this point.

## Other open defects, not part of the two bugs

These were surfaced while auditing the implementation lane for landing. They are real, they are
small, and they will be invisible again if they are not written down.

**A sealed-capability rail may be inert — and one of them provably is.** Sealed capability types
enforce nothing if they are never constructed. Verified first-hand:
`HostManageComponentMetaOutputCap` is constructed on `refactor` (at
`crates/verter_session/src/host_manage/component_meta_methods/macro_output_expansion.rs:120`) but has
**zero construction sites on `stage10/b6`** — the cutover lane removed the mint, so on the lane
heading to a landing that capability rail is **inert, and the guard built on it enforces nothing**.
Restore the mint or delete the type; do not land a decorative capability. (One correction to the
record: the same claim was made about `RegistryMemberShapeKeyCap`, and it is **wrong** — that one is
still constructed in production on both branches. Do not act on it without re-checking.)

**A CRITICAL rule's prescribed mechanism lost its only caller on the cutover lane.** The Typed-IR-Only
rule names `ResolverContext::workspace_is_workspace_owned` **by name** as the required alternative to
`node_modules` path-substring checks. On `refactor`, it has a real production caller: the shallow-
preserve path builds a `PolicyContext { is_workspace_owned, is_package_backed, … }` and calls
`imported_ref_must_materialize_canonically` through it. **On `stage10/b6`, that construction is gone —
`PolicyContext` is never constructed anywhere in `src`, and `workspace_is_workspace_owned` has no
production caller at all.** So the cutover lane **dropped the workspace-ownership policy seam** from
the symbolic-vs-materialise decision. That is a live rule violation on the landing lane, not a stale
rule. Restore the seam, or establish deliberately and in writing that the decision no longer needs
it.

**An unverified claim — do not repeat it as fact.** A reviewer asserted that the request-sticky
mechanism deleted during the cache work was an `Arc` crossing scheduler-worker threads, whereas the
thread-local tracer that replaced it is not — implying a lost cross-thread signal. **Nobody proved or
disproved it**, and if true it would apply to **both** independent implementations equally. Settle it
with a test that drives a demand across a scheduler worker boundary and asserts the non-cacheability
mark survives; do not carry it forward as a finding.

## Landing hygiene — one item is a correctness matter, not tidiness

- **The implementation lane must land as a SQUASH.** Its history contains commits that **do not
  compile** (the assembly merge `71a690b6b` is reported to fail with `E0599`/`E0277` in
  `crates/verter_semantic/src/facts/hashing.rs` — reported by a read-only diagnostic, not
  independently rebuilt). `git bisect` over that history is therefore **unsound**, and any
  "commit X was green" reasoning over it is unsafe.
- **`scratchpad/` was not gitignored** (verified: still absent from `.gitignore` on both `refactor`
  and `stage10/b6`; added on `mom/gate-green` in `165d6297f`). A `git add -A` swept 153 reviewer
  scratch notes into branch history. The tree is clean but the blobs are in history — which the
  squash also disposes of. **Land the ignore rule early.**
- Rebase onto current `refactor` and run the **full** workspace gate **before** the squash, not after.
- Scrub planning vocabulary from source comments and commit messages: the code reads as final state.
- **`cargo clippy --workspace -- -D warnings` must be green at landing.** `refactor` **is** green
  (verified by a read-only diagnostic, both `--workspace` and `--all-targets`). The implementation
  lane is **not**: 83 errors, all in `verter_session`, and — importantly — **present unchanged at the
  lane's own base**, so the lane introduced none of them. They are overwhelmingly orphaned
  `TypeExpr`-era readers whose last callers earlier cutover work deleted: **the dead code is
  precisely the residue this effort exists to delete**, so removing it is on-plan, not a workaround.
  Two cautions. First, hidden debt: because the `verter_session` library aborts under `-D warnings`,
  the crates downstream of it (`verter_ffi`, `verter_lsp`, `verter_mcp*`, `verter_wasm`,
  `verter_napi`) are **never clippy-checked** on that lane — more may sit behind the wall. Second,
  roughly thirteen of the 83 are **design-bearing and must not be blind-deleted** — the two items
  above are among them, and a third,
  `ProjectSemanticDispatch::replay_session_demand_to_hot` (on the lane at
  `crates/verter_session/src/project_semantic_dispatch/semantic_source.rs:1015`; it does not exist on
  `refactor`), is a **sanctioned** deferral: `CLAUDE.md` explicitly describes the hot-prepared
  carriers as dead-code-correct scaffolding whose wiring is deferred, so deleting it would contradict
  a stated architectural decision. **No blanket `#[allow(dead_code)]`** — it would neuter the
  shrinking ledger that tracks exactly this residue.
