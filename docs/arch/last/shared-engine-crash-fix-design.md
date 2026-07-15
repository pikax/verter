# The reachable stack-overflow crash in the shared projection primitive — implementer-ready design

**Status: IMPLEMENTED AND FOCUSED-GREEN (2026-07-14).** The recursive projection and deferred
evaluator staging have been replaced by explicit heap worklists. Operational exhaustion remains a
typed partial result, reaches the public diagnostic path, and is refused by warm memo/cache
admission. The release-wide gate and clean-history reconstruction remain tracked by
`release-consolidation-plan.md`.

## 1. The defect

`ProjectSemanticDispatch::project_view_node`
(`crates/verter_session/src/project_semantic_dispatch/locator_view.rs:357`; the uncached body at
`:375`) **recurses on the host stack, once per level of structural nesting, on every demand.**
Verified first-hand: it calls itself from every composite arm — tuples, unions, arrays, conditional
arms, instantiation-reference arguments, and most composites.

Because a Rust stack overflow **aborts the process** (`SIGABRT`), a sufficiently deep type does not
produce a partial result, a budget trip, or an error — it kills the process. At a default ~2 MB
stack, a throwaway diagnostic reproduced it: a regrowth type `type Grow<T> = T | Grow<[[[[T]]]]>`
crashed at roughly 33 successive demands, and a wider regrowth (`[[[[[[[[T]]]]]]]]`) at roughly 17 —
**both before any existing fuse trips** (these depths are reported from the diagnostic and not
independently re-derived; the recursion itself is verified). A single ~200-deep authored body
crashes on the **first** demand.

Three consequences that determine the shape of the fix:

- **It is in the shared primitive**, so it is reachable by **any** consumer — component-meta, the
  LSP, anything demanding a structural fact. It is not specific to the new work; the new work only
  stressed the primitive hard enough to surface it.
- **A reducer-local nesting bound is not a fix.** It bounds *regrowth*, but a single deep authored
  type crashes inside the shared primitive on first touch, below any reducer, at arbitrary `k`.
- **It cannot be deferred behind the cutover.** Deleting the second engine broadens the sole
  engine's exposure — from metadata and LSP surfaces to runtime and IDE/TSX codegen — so shipping
  that cutover over a reachable abort is strictly worse than shipping it today.

## 2. The fix — one explicit heap worklist over the whole function

Replace the host-recursive `project_view_node` / `project_view_node_uncached` with **one explicit
heap post-order worklist** covering the *whole* function — every structural arm **and** every
operator/reference arm. Each operator's `execute_type_node` re-dispatch becomes a **synchronous
leaf** from the worklist's point of view (it may cold-build its own worklist internally). The
consult considered and **rejected** a spine-only variant that would have left the operator arms
recursive: it does not close the crash.

The decided representation is a **two-stack staged interpreter**, monomorphic, with **no boxed
closures or trait objects**:

```
control: SmallVec<[ViewFrame; 16]>       // the active control stack
results: SmallVec<[SemanticNodeId; 32]>  // completed child results
```

`ViewFrame` is a compact `Enter { node, ctx }` plus variant-specific `Resume*` frames carrying
`{ memo_key, stage, result_base, owned Arc metadata }`. (A single `Vec<Frame>` plus an
`Option<SemanticNodeId>` last-result is acceptable if it satisfies the invariants below.)

After this rewrite there is **zero host-stack recursion** even for 200 nested conditionals. The only
residual host recursion is **cross-query mutual re-entry**, which rail B below is what bounds.

### The invariants — each is a semantics-preservation obligation

- **Each resume schedules EXACTLY ONE next child.** This is the load-bearing one: it makes
  control-stack depth proportional to structural **nesting**, never to union or object **breadth**.
- **`Enter`** does: the existing memo lookup; a watchdog beat on a miss; terminal handling; work
  charging (§3); cloning any required `Arc` payload; and **dropping the graph borrow before
  descending**.
- **Memoisation stays exact:** look up on `Enter`; **no** in-progress or tentative insert; insert
  `(node, ctx) → result` **only** at a complete finish or a leaf. **An operational partial must
  never insert.**
- **Exact depth-first left-to-right child sequencing, per arm.** Type parameters: constraint then
  default, with an identity early-return that yields the **original** node when children are
  unchanged. Unions and intersections: source order; empty collapses to never; singletons collapse —
  but a **merged declaration always re-interns as merged and never collapses**. Arrays: an unchanged
  element yields the original node. Tuples: project **all** elements first (with label, optional and
  rest information) and only then run spread-normalisation on intern — **never a plain tuple
  intern**. Template literals: expression order, no identity shortcut. Objects: members under
  structural provenance with stamps taken from the *original* context, then call signatures, then
  construct signatures, then each index (key then value), then the keyspace. Functions: parameters,
  then return, then each type parameter (constraint then default). Constructor types: await the
  signature. Aliases: await the target, return its result, memoise on the alias key. Raw fallbacks
  return the shared opaque-miss sentinel.
- **The data-dependent operator arms are STAGED** — a child's context derives from a prior child's
  **result**, not from the original node. Conditional: check → derive the extends-context from the
  check **result** topology → extends → true → false → leaf dispatch. Indexed access: derive the
  object context from the **original** object topology → object → index → decide deferral from
  **both** results → literal-fold **after** the index → dispatch or defer. Mapped: keyof-sourcing
  from the original topology → source → derive/execute the keyspace → value expression and name
  remap → classify the mapper kind → apply the open-carrier-stop test → possibly a leaf dispatch.
  `typeof`: resolve the root → namespace fallback → project the path **before** the type arguments,
  and a failed root or path must **not** project the arguments. `keyof`: project the base **before**
  dispatch, preserving deferred opaque and missing cases.
- **The lazy resolver heads must stay lazy.** Give the bare-reference and import-type head resolvers
  a private two-phase form — `prepare_*_head` returning either `Ready(node)` (no arguments
  scheduled) or `NeedsArgs(continuation)` — and only the latter schedules argument projection. The
  continuation resumes **once** with the ordered argument slice. **An unresolved head must NEVER
  project its arguments**: eager argument projection there is both a semantic and a performance
  divergence from today's closure-based laziness. Declaration references remain leaves (the
  active-identity check precedes the mode gates and resolution; non-eager modes return the original
  node **without** dispatching). Instantiation references project **all** arguments depth-first
  left-to-right under structural provenance before their gates, and rebuild to the original node
  when the projected slice equals the original.
- **`substitutions` and `memo` are single mutable values owned by the loop; frames hold no
  references to them.** `substitutions` is DFS-order-observable, so checkpoint its length at
  projector entry and **truncate back to the checkpoint on an aborted or partial projection** — a
  torn substitution set is a correctness bug, not a cosmetic one.

### Performance neutrality — it is a hot primitive

Retain the direct fast paths for a root memo hit and for terminal nodes **before** allocating any
worklist. One frame vector per body projection, small initial capacity, `SmallVec` inline so that
nesting below ~10 is allocation-free. Unary frames allocate no result vector; n-ary result vectors
use exact source capacity (the same allocation class as today's `map().collect()`). **No
thread-local scratch reuse** — the primitive is re-entrant and sharing scratch across re-entry is
unsound. Bench before accepting: terminal and memo-hit paths; depth 4 and 8 object/array/union;
32-arm and 1000-arm unions and intersections; full-child-group objects and functions; shared-DAG
context-split memo hits; the staged conditional/indexed/mapped arms; and resolvable **and
unresolvable** bare-reference and import-type heads — the last of these is where you confirm dead
arguments stay **unprojected**. Require no shallow allocation increase and no material
shallow-throughput regression, and **report both relative and absolute numbers**. Percentage-only
judgments are misleading for single-node nanosecond cases; production-shaped reference/operator
paths and contemporaneous baseline reruns decide materiality.

### Implemented evidence

- The frozen recursive primitive aborts its controlled 2 MiB-stack subprocess with Windows stack
  overflow `0xc00000fd`. This red evidence was captured in a protected detached worktree before the
  fix; the same authored 200-deep tuple and closed-conditional cases now exit normally and resolve
  `Complete`.
- Ten discriminating projection tests cover legitimate recursion, a genuinely pre-tripped active
  identity cycle, exact cooperative-memo identity, deep finite tuple/conditional structures, the
  exact work boundary, distinct connected-query depth exhaustion, runaway fresh-identity growth,
  recomputation/non-admission, and stable unresolved complete carriers.
- Public limit diagnostics use `verter/type-expansion-budget` and
  `verter/type-query-depth-limit`; each root demand/reason is deduplicated and carries the best
  available source span and safe carrier.
- `ProjectionFrame` is compile-time guarded at 32 bytes. Reference arguments reuse one boxed
  continuation state; mapped staging reuses one boxed state across stages. A fresh terminal
  projection allocates once and a root memo hit allocates zero times.
- A fresh alternating three-before/three-after 30-sample Criterion matrix (1 s warm-up, 2 s
  measurement) preserves the allocation snapshot at one allocation for a fresh terminal projection
  and zero for a memo hit. Medians of the three run point estimates show 32/1000-arm unions faster
  by 13.1%/19.2%; production-shaped resolved/unresolved bare and import references range from 5.5%
  faster to 7.0% slower (+639 ns at the largest positive absolute delta); conditional and indexed
  staging add 240 ns/133 ns; the shared-DAG case improves by 380 ns. The largest shallow
  percentages are the full object (+450 ns/+17.3%), full function (+184 ns/+14.8%), and mapped
  (+433 ns/+11.2%) microcases. Terminal cold and memo-hit paths add 7.4 ns and 2.1 ns respectively.
  Individual run medians vary materially on this Windows host, so acceptance uses these absolute
  costs and production-shaped paths alongside the relative figures rather than selecting one
  favourable percentage.

## 3. The fuse — dual rail, and deliberately NO structural-depth cap

A structural or worklist-depth cap was **explicitly rejected**: it would re-introduce "reject a
legitimate finite input because it happens to be deeply nested", which is precisely the failure mode
being removed. Instead, a dispatcher-owned, **RAII-scoped**, structurally-confined state (private
type, module visibility, brief cell access — **never** a mutable borrow held across a query, and a
panic-safe root-scope reset), installed by the **outermost** entry that lacks one and **joined** by
every nested dispatch, so that one connected demand shares one state. Both rails are budget-free —
they fire with no request budget present:

- **Rail A — a total WORK ceiling for one connected demand.** Charge one unit per worklist `Enter`
  (memo probes included), one per synchronous operator/resolver leaf dispatch, one per evaluator
  frame or iteration, and one per instantiate-build entry. Reuse the existing ~262,144 envelope. A
  trip yields a typed `PROJECTION_WORK_LIMIT` partial. This is what bounds a **single deep demand's
  internal work** — which a per-demand reducer fuse structurally cannot see.
- **Rail B — a cross-query HOST-RECURSION depth cap.** This is the **only** host-stack rail that
  survives. Cap *genuinely recursive host boundaries* only: an operator/resolver dispatch that
  nests another cold query/build/project chain. Heap evaluator entries and worklist frames are not
  query-depth boundaries. **Do not cap worklist or structural depth.** Start at 24 and **calibrate
  empirically against the 2 MB
  controlled-stack test using the worst observed cross-query build path, retaining substantial
  headroom** for frame-size drift, diagnostics and unwind-drop (a defensible band is 8–32). A trip
  yields a typed `CONNECTED_QUERY_DEPTH_LIMIT` partial.

## 4. Completeness integration

The iterative driver returns a private `ProjectedViewOutcome { node, completeness }`. On a work or
depth trip it produces a **carrier-stop node plus a typed `Partial`** carrying the tripped reason;
reasons merge. The outcome is consumed **centrally** at the body-projection choke point
(`lower_located_body_with_provenance`, whose ~9 call sites were verified to be the real choke —
not the instantiate builder alone) through the existing build-scoped fold, which folds the
completeness into the active demand state and taints the build frame so that the result **refuses
memo and cache admission**. A trip must **never** fabricate a `Complete`.

The evaluator's recursive deferred `keyof` / indexed-access staging converts to **heap frames** by
the same rule. The former 256-deep evaluator ceiling is retired completely: it is neither a
host-stack rail nor a logical structural-depth limit. Its compatibility reason bit remains reserved
but has no production producer. The identity guard that maps an exact same-identity cycle to
`Opaque(RecursiveRef)` **stays first and unchanged**. Be precise about reasons: resource exhaustion
is **not** a miss, a connected-query depth trip is **not** same-path recursion, and a stable
unresolved reference or authored miss stays **`Complete` on its carrier**.

### 4.1 Public diagnostic contract

Operational partiality is user-visible, not telemetry-only. The typed rails propagate through
`ShallowDiagnostic` and `ExpansionStopReason` into component-meta and the LSP diagnostics path:

- `PROJECTION_WORK_LIMIT` / `ProjectionWorkLimit` maps to
  `verter/type-expansion-budget` with “Type expansion exceeded Verter's safe evaluation budget.”
- `CONNECTED_QUERY_DEPTH_LIMIT` / `ConnectedQueryDepthLimit` maps to
  `verter/type-query-depth-limit` with a distinct connected-query-depth message.

Both are warning diagnostics. The mapper attaches the authored macro/root span when available and
deduplicates by root demand plus typed reason, so one runaway root produces one public diagnostic.
The best safe carrier remains available in the partial result, but neither the deferred memo, the
shared query memo, nor a final component-meta cache may admit it; a repeated demand recomputes.
Exact in-flight identity cycles retain their recursive sentinel semantics and do not produce either
budget diagnostic. Legitimate recursive carriers, deep finite structures, and stable unresolved
references remain complete and diagnostic-free. Logs and audit events are supplementary only.

**After this change, no correctness path may depend on `RUST_MIN_STACK`.**

## 5. Legacy deletions — one clean cutover, no shims

The recursive `project_view_node` / `project_view_node_uncached` bodies; the recursive evaluator
staging; and the 256-ceiling's host-stack-rail role plus the 128 MB sizing rationale. No dual path,
no feature flag.

## 6. The regressions MUST run in a 2 MB-stack subprocess

This is the part that is easiest to get wrong and that invalidates everything if you do.

A stack overflow aborts the **whole process**, so the test must isolate it in a **subprocess**: the
parent re-invokes the test executable under a child-mode marker; the child **removes
`RUST_MIN_STACK` from its environment** and then spawns the demand inside a thread built with an
explicit **2 MB** stack; the parent asserts a clean child exit **and** the expected typed outcome.
Prior art exists in `crates/verter_session/src/component_meta_pathological_recursion_tests.rs` and
`crates/verter_session/tests/cases/g_misc2/pe4_evaluate_depth_budget.rs`. Respect the
one-test-binary-per-crate layout: wire new cases through `tests/cases/` and `main.rs`, or place them
in-crate — **never** a new top-level `tests/*.rs`.

> **The workspace's `RUST_MIN_STACK=128MB` HIDES this crash.** A crash regression that runs under
> the ordinary test environment proves nothing. **If a "crash" test does not crash the pre-change
> primitive, your harness is wrong — fix the harness. Do not accept the green.** Capture the RED-pre
> evidence (the actual abort signal at 2 MB) for every case before you write the fix.
>
> One caveat you should settle rather than inherit: I could not find `RUST_MIN_STACK` set anywhere
> in tracked configuration — not `.cargo/config.toml`, not `.config/nextest.toml`, not `scripts/`,
> not the CI workflows. It appears **only inside source comments**. Either it is an ambient
> developer/CI environment variable, or the sizing rationale is stale and the real budget is the OS
> default — in which case the crash is **more** reachable in an ordinary run, not less. Neither
> reading changes the harness, which pins its own 2 MB stack explicitly.

### What the suite must pin — both directions

Crash-freedom, each RED-pre at 2 MB and GREEN post-fix:

- A 200-deep authored array/tuple body resolves **`Complete`**.
- **200 nested closed conditionals** in one body resolve to the exact expected node and **`Complete`**
  — this is the case that discriminates the full-function rewrite from a spine-only one.
- Regrowth (`Grow<T> = T | Grow<[[[[T]]]]>`, and 8-, 140- and 200-deep nestings), driven **directly
  through successive instantiate demands**, does **not** abort: fresh-identity growth returns a
  typed `Partial(PROJECTION_WORK_LIMIT)`, **refuses warm admission**, and a repeat **recomputes**.
- A chain of **more than `MAX_CONNECTED_QUERY_DEPTH` distinct declarations** through operator
  re-dispatch returns `Partial(CONNECTED_QUERY_DEPTH_LIMIT)` with **no cache admission**.
- A projection-budget trip after some children have completed leaves the unfinished root in
  **neither** the view memo **nor** the shared query memo.
- Boundary controls **at** the depth cap and at **+1**, and the work ceiling at and over the final
  permitted step (a result that terminates on exactly the last permitted step must **not** be falsely
  partial).

Anti-over-partialisation, which is exactly as load-bearing as crash-freedom — a fix that turns
legitimate types into partials has traded a crash for silent wrongness:

- A deep-but-finite structural type, well beyond ordinary nesting but below the work envelope,
  resolves **`Complete`**. This is what proves there is **no** structural depth cap.
- A same-identity recursive reference keeps its existing `Opaque(RecursiveRef)` behaviour, unchanged.
- A **stable missing or unresolved declaration reference stays `Complete` on its original carrier —
  never `Partial`.** (Mandatory.)

### Differential characterisation — green before, green after

Capture golden values by running against the **pre-rewrite** primitive, encode them as exact literal
assertions, and confirm they stay green after the rewrite. Cover: tuple-spread normalisation;
identity preservation for type parameters, arrays and instantiation references (unchanged children →
the original node); the staged conditional, indexed-access and mapped contexts; and the lazy
bare-reference and import-type heads — **explicitly verifying that unresolved heads leave their
arguments unprojected** — together with the observable ordering of `substitutions`.

## 7. Sequencing

Do this **after** the cache-admission closure
([`cache-admission-closure-design.md`](cache-admission-closure-design.md)) and **before**
reintroducing the reducer. An earlier work-in-progress version of that reducer was built against the
**recursive** primitive and carried a stack-safety claim that is **empirically false**; it was
discarded and **no longer exists anywhere**. That is fine — it should not be resurrected in any case.
Rebuild the reducer against the new primitive, where its only mechanical hazard is adapting to the
`Complete` / `Partial` outcome, and do not re-inherit its false no-budget stack-safety assumption.
