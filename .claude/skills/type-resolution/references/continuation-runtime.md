- `SemanticExecution` (`semantic_execution.rs`) — the continuation runtime: a request-scoped demand table (the execution's completed results), a frame arena and a ready queue driven by one loop. A frame needing another demand returns `EvalStep::Need` and is parked; completion enqueues its waiters and never resumes one recursively, a need of an open demand on the frame's own chain is a cycle the program answers, and a frame's step never drives a nested execution. An instantiation or a `typeof` evaluated outside any drive runs on it (`project_semantic_dispatch/query_frames.rs`): its build is a frame whose decl-body projection needs each instantiation it reaches instead of evaluating it in place, so an alias chain is a chain of frames bounded by neither the stack nor the native nested-query depth. A decl-body projection that reaches a value's `typeof` (no member path, no type arguments) needs it the same way, and that `typeof` builds as a frame of its own when the value's type is its authored annotation (`begin_typeof` / `drain_typeof`): the annotation's projection needs each instantiation it reaches, so `typeof __v` over `{ [K in keyof L99<0>]: L99<0>[K] }`, whose interface heritage nests a hundred levels, is a hundred frames. A frame owns its lease, tracer and build-local taint, installs them only for its own steps, replays each delivery's carrier into its tracer and folds its rails into its taint; code a step reaches synchronously evaluates in place and never drives.
- A root `typeof` drives like an instantiation, and every `typeof` build that evaluates something its value's type comes from suspends on the runtime instead of evaluating what it reaches in place (`TypeofBuild`, `drain_typeof`):
  - a value whose type lowers from its authored value position — its annotation, its inferred initializer, its const-object shape (`lower_typeof_body_or_suspend`) — lowers it as a projection that needs each instantiation and `typeof` it reaches, so `declare const v<i>: typeof v<i-1>` and `const x<i> = x<i-1>`, 500 deep, are 500 frames on a 1 MiB stack;
  - a function value lowers its signatures one at a time (`SignatureLowering`): its own declaration's, then each merged declaration's (a later script's global function, a `declare module` block's) read as its turn comes, so `declare function h<i>(): ReturnType<typeof h<i-1>>` is a chain of frames; the augmenting blocks' overloads of an exported function still lower in place;
  - a value initialized by an expression source evaluates that expression as the frame's build (`ExpressionSourceRun` over `IndexedValueRun`): the indexed evaluation's explicit stack names its position by the children it descended through, never by borrowing the expression, and stops at each leaf that is a value's `typeof` (`id(y)` reads `typeof y`) with that `typeof` as its need, so `const y<i> = id(y<i-1>)` is a chain of frames. The inline evaluator runs the same run to completion. The probe the inline path wraps the evaluation in is one observation across the steps: each step re-opens it with what the earlier steps observed (`ObservedRead::resume`) and consumes its delivery inside it (`FrameDelivery`), and the last step adopts it when the evaluation yields a value and drops it when it yields none. A leaf of any other shape (a structured value literal naming values) still lowers in place.
  Conditional decisions that normalize their operands (`decide_conditional` → `normalize_node_for_structural_fact_demand` → an in-place `Instantiate`), and a function signature whose body-derived return reads through `LowerLocator` into an in-place `FlowReturn`, still nest natively.
- Driving is by polls. `SemanticExecution::poll(program, root, waker)` runs every runnable frame and returns `Ready` with the root's outcome (or the stop that abandoned every open demand), or `Pending` once the chain's tip waits on an outside producer; later polls of the same drive pass no root. `Program::poll_subscription` checks the producer and registers the waker in one atomic step. For the memo this is `Subscription::poll` (`semantic_query_memo/producer.rs`): the readiness check and the waker registration happen under the flight's lock, and every flight resolution (completion, abort, cancellation sweep, invalidation, a panicking producer) goes through `FlightCell::wake_subscribers`, which notifies the condvar and wakes every registered waker after resolving under that lock, so a resolution between a poll's check and its registration is never lost. A pending subscriber holds its wait-for edge (cycle refusal unchanged) and its waker until it resolves or drops; dropping a pending subscription, or a pending execution, releases the edge, takes the waker off the flight and drops every suspended frame (leases abort, active instantiations leave, open cost scopes abandon). `drive` is the blocking driver: it polls with a per-thread unparking waker and parks between polls in 2 ms slices, the same cancellation-observation rail the blocking condvar wait keeps. The synchronous (non-driven) entry keeps `Subscription::wait`.
- An execution's state is `Send` whenever its program's frames, demands, values and subscriptions are; `poll` takes it exclusively, so one driver steps it at a time. A request context bound with `bind_request_context` is installed through `RequestContextLike::install_tls` for each step and the stepping thread's own restored after it. The dispatcher's `QueryProgram` frames still borrow the thread-bound dispatcher, so the production drive is polled on the thread that began it and binds no context.
## Semantic Query Identity Target Contract

### Exact symbol-identity demand for typed feature roles

`ProjectSemanticDispatch::demand_symbol_identity` is the shared adapter for
feature facts that require declaration identity. It walks existing graph
carriers and delegates declaration bodies to the canonical `ResolveDecl` and
`Instantiate` queries; it must not grow a parallel alias/import resolver or
parse terminal text. Direct, imported-alias, and multi-hop local-alias subjects
therefore retain the dispatcher's fixed view, dependency signatures,
cycle/budget/work limits, and cache-suppression behavior.

The outcome is closed: complete match/non-match returns an optional
`ResolvedSymbolIdentity`; partial returns only a typed
`PropCallableRoleUnresolvedReason`, never a graph node. Every partial outcome
folds into cold-compute completeness and is ineligible for exact warm
admission. Svelte `Snippet` role classification compares this identity only
with the package-validated `svelte` import fact.

Architectural target for the project-global cache cutover:

- Type expansion has one authoritative semantic query path.
- Query memoization keys must be semantic and scope-aware, not request-local and not raw-text-based.
- Request ids are not query ids. They must not be the primary dedup key for reusable type work.
- Reusable semantic operations (resolved declaration lookup, indexed access, member projection, instantiation, mapped-type application, conditional-type branches) enter through shared query-key types.
- Bare-name lookups must include the declaration scope or resolved root identity needed to avoid cross-scope poisoning.
- Qualified namespace lookup is a two-stage exact-owner route: prove `(owner, namespace_alias)` from cached shallow import facts as a MODULE handle, then resolve the qualified MEMBER through the shared type/value export resolver. Never probe the dependency for an export named after the local namespace alias, and never stop a re-export at its barrel identity; ambiguous namespace bindings fail closed.
- Semantic query-identity keys are content-free (R6): a resolved declaration or route identity carries semantic identity plus the split env dimensions only, never a content/version hash, whole-hash, or `fact_dep_signature`. Version-rooting lives EXCLUSIVELY on the cached value (`ReadSetSignature.facts` + `self_root_canonicals`, revalidated on every warm read); the live content version (whole-hash) is re-sourced at value-compute time (`ensure_indexed_ready_serve`), never carried in the key.
- Semantic nodes are immutable. File changes create new identities rather than mutating old ones in place.
- The shared semantic layer is a host-owned memo table keyed by semantic query identity. Any ID-backed semantic graph behind it is secondary and must store immutable AST-free semantic data rather than borrowed OXC pointers.
- Same-file shallow closure may run inside the winning query, but reusable cross-file and projection work should be represented as dedupable semantic subqueries.
- Recursive and mutually-recursive expansion must use an explicit in-flight table so one winning query computes each cold semantic node and recursive re-entry dedups cleanly.
- Same-path recursion must never self-await. If the same execution path sees a `Running` semantic node again, return the solver's normal recursion sentinel / unresolved recursive form instead of blocking on itself.
- Distinct top-level callers encountering a `Running` semantic node should wait cooperatively on a completion primitive rather than spin-retrying.
- Nested semantic builders may also wait cooperatively after releasing short-lived memo guards or locks. Do not busy-spin, and do not make whole-stack unwind a required waiting strategy for ordinary DAG dependencies.
- Cooperative waits are cycle-safe across tasks: each semantic execution is a
  store-local generation-qualified task that owns the producers it claimed,
  and each parked subscriber holds one RAII `waiter → producer` edge. An edge
  that would close a wait-for cycle returns the established recursion carrier
  through `ReturnOnly` (partial + nonpublishing) instead of parking. Task,
  producer and edge cleanup is RAII on normal return, cancellation, and panic;
  generation checks make stale cleanup harmless after a task slot is reused.
  This is orthogonal to same-task same-path recursion, which keeps its
  sentinel behavior and is checked before wait registration.

Concrete expectation:

- If one larger expression references `C`, `C['foo']`, `C['bar']`, and `B`, and `B` itself references `C` again, the resolver should converge those onto one shared semantic query graph rather than recomputing each path ad hoc.

### Vue Runtime Surface And Broad Runtime Classification

Framework event projection uses Canonical Resolved Emit Occurrences (CREO).
The resolver walks one ordered `SurfaceEntry` stream spanning members, call
signatures, construct signatures, and index signatures, and derives kind
indexes from it. Event-producing entries expand directly into complete
`ResolvedEmitOccurrence` rows. Identity is the exact authored origin plus the
instantiated semantic subject and event-name arm: identical diamonds dedup,
different generic instantiations and distinct declarations remain distinct.
Consumers must not reconstruct membership or order from kind arrays, names,
analyzer rows, or positional counters.

Vue runtime props/emits uses the canonical semantic-query graph, never a
request-local aggregate cache. Its internal
`ReductionDemand::VueRuntimeObjectSurface` is an internal demand/memo-slot
selector, not a sixth `ProjectionMode` and not a wire API. Its constructor also
sets the orthogonal content-free
`VueHeritagePolicy::SuppressIgnored`; TSC, component-meta, slots, and every
ordinary context default to `RetainAll`. The runtime demand has the same
Shallow union-of-members and operator-reduction semantics as
`MacroObjectSurface`, while the policy removes only producer-addressed
`PreparedTypeDecl.vue_ignored_heritage` arms before substitution, heritage-head
resolution, or merging. Declaration-carrier unwraps demote the demand to
`StructuralTransit` to retain carrier-stop semantics but MUST preserve the
policy. Any reducer that intentionally creates fresh mode/demand/provenance/
merge-role semantics from an active context MUST use
`ProjectionReductionContext::with_orthogonal_axes_from`; this is the sole
policy-inheritance adapter and exhaustively classifies every context field.
PathWalker routes every fresh context through one template constructor;
full-axis carrier demotions use `into_structural_transit_with_mode`. Mapped-source
enumeration remains ordinary `Published(Shallow)` (TS intersection semantics)
and inherits only the policy. Every
`ProjectionReductionContext`-bearing family and mapped-member context encoding
carries the policy, so filtered/unfiltered transit values cannot cross-serve;
the two publication demands additionally occupy independent non-backfilling
slots. Versioning remains the existing value-side `FileWholeHash`/read-set
validation, overlay candidates, and singleflight.

`ClassifyBroadRuntime` is the sole broad constructor classifier. It traverses
aliases/unions/intersections iteratively in source order, recognizes nominal
builtins before structural expansion, and treats Object/Function/Array as
terminal broad facts without enumerating members, sibling declarations, or
nested object bodies. Missing graph data, recursion, cancellation/work-budget
exhaustion, unstable state, and non-Miss query faults return typed `Partial`
with explicit `Unknown` and `ReturnOnly`; honest semantic unknown/Miss remains
distinct and may be `Complete`. Only content-free canonical macro
payload/member locator subjects may warm the shared family memo;
the build re-sources the live node from the current indexed artifact and
value-side read set. Anonymous graph-instance subjects use the explicit
transient path and remain ReturnOnly even when a descendant is file-rooted.
`SemanticNodeId`, content hashes, spans, and source/rendered text are forbidden
in the durable classifier family identity.

`ClassifyTruthinessDomain` is the sole truthiness-domain authority, owned by
the canonical semantic-types layer (`project_semantic_dispatch::canonical_algebra`)
and CONSUMED by the flow narrowing truthiness frame, which holds no private
truthiness rule. It is a demand-scoped structural query over one interned
subject node returning per-bucket trileans (`TruthinessDomain { truthy, falsy }`,
each `Yes | No | Undecided`): union arms OR; intersections are `never`-dominant
then OR (checker-measured: `string & {x: 1}` keeps the falsy edge, `{a: 1} & {b: 2}`
leaves it); a template-literal type contains `""` iff every quasi is empty and
every placeholder can render empty — only a `string` placeholder can:
`${number}`/`${bigint}`/`${boolean}`/`${null}`/`${undefined}` always render
non-empty, and the checker treats `${any}` as non-empty too (measured: its
falsy edge is `never` even though `""` is assignable to it); a `never`
placeholder empties the template; a memberless `{}` keeps both buckets while any-membered surfaces,
arrays, tuples, and signatures are truthy-only; a type parameter classifies
through its constraint (`unknown`'s domain when unconstrained). It never
resolves references or inlines aliases — an unresolved carrier is `Undecided`,
reported, never guessed, and only a FULLY decided domain admits into the family
memo (an `Undecided` bucket or incomplete walk is `ReturnOnly`). The flow
consumer settles each arm through the shared identity-carrier unwrap, keeps an
arm whose tested bucket is not `No`, records the typed `GuardNarrowing` gap on
`Undecided`, and narrows the subject to `never` (branch alive, syntactic
returns retained) when no arm survives an edge.

