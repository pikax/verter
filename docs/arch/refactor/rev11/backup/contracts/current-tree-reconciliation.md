# Current-Tree Reconciliation Contract

**Status:** Normative implementation-entry contract.  
**Purpose:** prevent a neutral target name from accidentally creating a second owner beside a correct current owner.

# 1. Required disposition

Each affected current authority receives exactly one disposition:

- **Preserve** — already owns the final invariant; adapt callers/tests only as necessary.
- **Converge** — survives but absorbs/removes adjacent duplicate responsibility.
- **Replace** — a new owner is justified; old owner and every caller are deleted/migrated in the same accepted cutover.
- **Delete** — responsibility is unnecessary or already owned elsewhere.
- **Defer** — outside the current block; block must prove it does not depend on changing it.

A row marked `VERIFY` blocks the affected implementation block.

# 2. Mandatory row schema

| Surface/current symbol | Source paths | Current invariant/authority | Direct readers/writers/callers | Lifetime/thread boundary | Cache/protocol compatibility | Target disposition | Final owner | Exact deletion/migration set | Proof block | Status |
|---|---|---|---|---|---|---|---|---|---|---|

# 3. Seed inventory from the historical `9af…` evidence baseline

The following rows are hypotheses derived from the historical evidence baseline. Each must be source-verified and expanded against the exact `A0` checkout; they are not claims about current `main`.

| Surface/current symbol | Candidate current authority | Revision 11 constraint | Initial status |
|---|---|---|---|
| open PRs/branches/queued changes touching an architecture owner | parallel architecture-affecting work | include, exclude, abandon, or coordinate before baseline lock; no unaccounted competing rewrite | VERIFY |
| registered source/VFS/`PublishedRoot`/workspace snapshot | host-backed source, project, invalidation, and publication basis | preserve or converge into the single committed-input role before QueryRuntime convergence; do not create a second `InputStore` by name alone | VERIFY |
| `verter_session::resolver_core` / `ProjectSemanticDispatch` | shared host-backed module/type-resolution orchestration | preserve one resolver semantics path; extraction may change dependency direction but cannot create a second resolver | VERIFY |
| `IndexedReady` and shallow symbol inventory | canonical shallow declaration/index artifact | preserve demand-driven broad shallow index if source proof matches; no rescanning to rediscover indexed facts | VERIFY |
| `DeclBodyMemo` / retained parse workers / `DeclLoweringService` | lazy body lowering over retained parse snapshots | reconcile into managed parse owner domains; direct compiler remains independent | VERIFY |
| `ProjectTypeStore`, `RouteDb`, fact/read-set caches | current query/cache families | classify each cache separately; preserve value-side validation where correct; delete duplicate ownership only after proof | VERIFY |
| `SemanticGraphStore` and component-meta materialization caches | managed semantic/component-meta storage | reconcile lifetime, cohort, lock, admission, and current native/compat consumers before the public TypeExpr/operation-DTO cutover | VERIFY |
| `FunctionProgramIndex`/`FunctionFlowGraph` | canonical flow structure | PRESERVE unless source evidence disproves; extend same graph only | VERIFY |
| `flow_slice_content.rs` syntax-shaped evaluator | second flow/control semantics path | REPLACE/DELETE through final flow blocks; do not port it as a new IR | VERIFY |
| `CodeTransform` | code plus mapping transformation authority | preserve atomic code/mapping semantics and reuse in compact source-unit cutover | VERIFY |
| `StyleSyntaxIr` and current fast CSS paths | CSS-family syntax/transform substrate | preserve one syntax authority; do not delete a proven specialized fast path without equivalent-work evidence | VERIFY |
| component-meta native/compat boundary | product-facing compatibility behavior | inventory consumers/oracles and migrate after final semantic/flow plus the affected consumer identity/lifetime/admission contracts; no silent behavior merge | VERIFY |
| ProviderHub/SyncCoordinator/provider actors | external TypeScript lifecycle/synchronization | preserve stateful actor ownership where required; converge stamps/readiness, never race providers | VERIFY |
| `VerterHost` / session facade | current public/catch-all entry owner | reduce only after every extracted invariant has a complete owner and consumer migration | VERIFY |
| `TypeExpr` producers/consumers and TypeInfo protobuf graph | current internal/public/wire contracts | consumer-by-consumer disposition after flow/query foundations; wire obligations explicit | VERIFY |
| audit TLS/substrate/runtime | deterministic optional observability | preserve leaf dependency direction and prove disabled overhead; do not make audit semantic authority | VERIFY |

# 4. Completion rule

Before a block enters `BLOCK_READY`:

- every touched row is resolved and linked to source evidence;
- every direct consumer is named, not represented by “and others”;
- the surviving owner and dependency direction are explicit;
- compatibility/wire implications are classified;
- the exact old declarations/caches/tasks/tests/docs to delete are listed;
- current behavior that is intentionally preserved has a characterization test;
- unresolved adjacent rows are proven outside the block's causal closure.
