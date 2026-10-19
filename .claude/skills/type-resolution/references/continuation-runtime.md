- `SemanticExecution` (`semantic_execution.rs`) — the continuation runtime: a request-scoped demand table (the execution's completed results), a frame arena and a ready queue driven by one loop. A frame needing another demand returns `EvalStep::Need` and is parked; completion enqueues its waiters and never resumes one recursively, a need of an open demand on the frame's own chain is a cycle the program answers, and a frame's step never drives a nested execution. An instantiation or a `typeof` evaluated outside any drive runs on it (`project_semantic_dispatch/query_frames.rs`): its build is a frame whose decl-body projection needs each instantiation it reaches instead of evaluating it in place, so an alias chain is a chain of frames bounded by neither the stack nor the native nested-query depth. A decl-body projection that reaches a value's `typeof` (no member path, no type arguments) needs it the same way, and that `typeof` builds as a frame of its own when the value's type is its authored annotation (`begin_typeof` / `drain_typeof`): the annotation's projection needs each instantiation it reaches, so `typeof __v` over `{ [K in keyof L99<0>]: L99<0>[K] }`, whose interface heritage nests a hundred levels, is a hundred frames. A frame owns its lease, tracer and build-local taint, installs them only for its own steps, replays each delivery's carrier into its tracer and folds its rails into its taint; code a step reaches synchronously evaluates in place and never drives.
- A root `typeof` drives like an instantiation, and every `typeof` build that evaluates something its value's type comes from suspends on the runtime instead of evaluating what it reaches in place (`TypeofBuild`, `drain_typeof`):
  - a value whose type lowers from its authored value position — its annotation, its inferred initializer, its const-object shape (`lower_typeof_body_or_suspend`) — lowers it as a projection that needs each instantiation and `typeof` it reaches, so `declare const v<i>: typeof v<i-1>` and `const x<i> = x<i-1>`, 500 deep, are 500 frames on a 1 MiB stack;
  - a function value lowers its signatures one at a time (`SignatureLowering`): its own declaration's, then each later script's merged global declaration read as its turn comes, so `declare function h<i>(): ReturnType<typeof h<i-1>>` is a chain of frames; the augmenting blocks' overloads of an exported function still lower in place;
  - a value initialized by an expression source evaluates that expression as the frame's build (`ExpressionSourceRun` over `IndexedValueRun`): the indexed evaluation's explicit stack names its position by the children it descended through, never by borrowing the expression, and stops at each leaf that is a value's `typeof` (`id(y)` reads `typeof y`) with that `typeof` as its need, so `const y<i> = id(y<i-1>)` is a chain of frames. The inline evaluator runs the same run to completion. The probe the inline path wraps the evaluation in is one observation across the steps: each step re-opens it with what the earlier steps observed (`ObservedRead::resume`) and consumes its delivery inside it (`FrameDelivery`), and the last step adopts it when the evaluation yields a value and drops it when it yields none. A leaf of any other shape (a structured value literal naming values) still lowers in place.
- A suspending projection that reaches a conditional type needs its decision (the `Conditional` query) instead of deciding it in place (`ProjectionSeam` holds the awaited decision and hands its delivery back to the conditional that needed it). The decision runs as a frame of its own (`ConditionalBuild`, `drain_conditional`): the decision procedure is a function of its operands and of the answers to the instantiations and `typeof`s it reads, so a step runs it with the answers its frame holds (`DecisionSeam`, `read_at_decision_seam`). A read the answers do not hold enters exactly as the frame's need would: a result the memo serves warm answers it at once and joins the answers; one that must be computed becomes one of the frame's needs, its claim continuing in `QueryProgram.retries`, and reads as a typed gap. The run collects every such need it reaches — a run that waits on a read (`decision_waits`) reads on through the operand normalization's per-arm and per-position walks instead of stopping at their first gap — then is dropped with every observation it made; the frame needs each collected read in turn and runs the decision again once all are delivered, so a decision over a 1,000-member union runs twice, not once per member. A query the decision evaluates in place (a relation, a reduction) reads only the answers (`nested_decision_query`): what it builds is kept, so it never reads a gap. Before relating, the decision demands the operands its relation unwraps — as the checker instantiates a conditional's check and extends types before relating them — so the relation reads them answered instead of instantiating them in place. A decision nested in the drive outside every checker obligation completes for the request alone, as it does nested in a synchronous build (`FrameProducer::RequestLocal`, `completes_request_locally`): it enters no family memo.
  A frame's result never nests natively below the code that reads it, so its receipt's nesting is carried apart from the reader's own (`OperationFootprint.frame_query_depth`): a reader's admission checks its own depth against the deeper of the two, and a chain of decisions does not stack a query level per link. `type C<i> = C<i-1> extends string ? … : never`, 500 deep, decides on a 1 MiB stack. A branch that selects the alias before keeps it as a carrier, and each level's decision walks the carriers below it: that walk, not the runtime, is quadratic in such a chain's length.
  A decision instantiates the operands that name declarations before it relates them, so the instantiation — and the flows its class methods' returns read — runs ahead of the relation, outside its component; a relation whose own walk instantiates them (operands held in object members) keeps them in its component.
  A suspending projection that reaches a member-path or applied `typeof` (`y.v`, `typeof f<string>`) needs it as it needs a value's own `typeof` (`ReferenceProjectionPlan::Typeof`, `ProjectionFrame::AwaitTypeof`). Its delivery carries whether the read answered a value and with which fact status (`ProjectionDelivery`): a read that answered none is a miss, and an applied `typeof` applies its type arguments to the answer; an inline run reads it in place (`read_typeof_in_place`). The read is one root-and-path `TypeOf` query, the lookup contract the in-place lowering keys through `typeof_reference_key`: a member path that answers no value is never retried under a joined, namespace-qualified name, which would bypass the module's export domain. A member-path `typeof` built as a frame lowers the member positions where the value declares them — an object literal's own members, a class's own statics — as its own suspending projections (`TypeofThen::Member`, `MemberLowering`, `value_member_plan`), so `const y<i> = { v: y<i-1>.v }` and `class C<i> { static v = C<i-1>.v }`, 500 deep, are chains of frames on a 1 MiB stack. A function signature whose body-derived return reads through `LowerLocator` into an in-place `FlowReturn` is bounded per signature, not by the chain.
- Driving is by polls. `SemanticExecution::poll(program, root, waker)` runs every runnable frame and returns `Ready` with the root's outcome (or the stop that abandoned every open demand), or `Pending` once the chain's tip waits on an outside producer; later polls of the same drive pass no root. `Program::poll_subscription` checks the producer and registers the waker in one atomic step. For the memo this is `Subscription::poll` (`semantic_query_memo/producer.rs`): the readiness check and the waker registration happen under the flight's lock, and every flight resolution (completion, abort, cancellation sweep, invalidation, a panicking producer) goes through `FlightCell::wake_subscribers`, which notifies the condvar and wakes every registered waker after resolving under that lock, so a resolution between a poll's check and its registration is never lost. A pending subscriber holds its wait-for edge (cycle refusal unchanged) and its waker until it resolves or drops; dropping a pending subscription, or a pending execution, releases the edge, takes the waker off the flight and drops every suspended frame (leases abort, active instantiations leave, open cost scopes abandon). `drive` is the blocking driver: it polls with a per-thread unparking waker and parks between polls in 2 ms slices, the same cancellation-observation rail the blocking condvar wait keeps. The synchronous (non-driven) entry keeps `Subscription::wait`.
- An execution's state is `Send` whenever its program's frames, demands, values and subscriptions are; `poll` takes it exclusively, so one driver steps it at a time. A request context bound with `bind_request_context` is installed through `RequestContextLike::install_tls` for each poll — every step and program callback it makes — and the polling thread's own restored after it; a thread already running under that request keeps what it installed.
- Every drive is a `SemanticTask` (`query_frames.rs`), the concrete production task, and it is `Send`. `drive_query` lends it the evaluating dispatcher's request state (`lend_semantic_task`): the task owns a task-scoped dispatcher (`task_dispatcher`) that shares the request's immutable inputs and receives, by `exchange_request_state`, every active instantiation and recursion guard, the checker transaction with its inference sessions, the build-taint stack, the connected demand with its cost recordings (`ConnectedDemandLedger::exchange_state`) and the request's kept results. The task returns that state when it is done or the drive unwinds (`LentTask`, `return_semantic_task`). Frames hold no dispatcher borrow: they name the state they entered, and the program releases it when the runtime abandons a frame (`Program::abandon`, `SemanticExecution::release`). The thread-bound construction scope belongs to the thread handle a dispatcher is created as (`ThreadDispatch`), not to the dispatcher. A task roots what it constructs in its own `TaskConstructionOwner`, and on return hands it to the construction owner open on the returning thread. Each poll installs, on the polling thread for the poll's extent, the task's construction owner, its execution task (`ExecutionScope::install`, on the store's one `TaskRegistry`), its execution validity (`TaskExecutionValidity`) and its bound request context. The request's shared inputs are `Sync` (`ResolverContext`, `FactValidation`, the request lifecycles, clocks and host attachment), and a fact read set is `Send` but not `Sync`, so a suspended task's own tracers move with it. One exclusive poller may therefore resume a pending task on any thread. `semantic_task_tests` resume one on another thread across `Pending` to the blocking answer, and release or drop one while pending with nothing left open.
- A call an initializer's indexed evaluation reaches inside a frame's step, when its resolution is a machinery root (`call_resumes_on_the_runtime`: no obligation open, no provisional answer), is the run's need (`IndexedNext::AwaitCall`) and resolves as a frame of its own (`FrameBuild::Call`, `CallBuild`, `drain_call`) through the same family memo claim, admission and publication the synchronous root takes; a completed walk's admitted publication drains the members it completed, as `execute_resolve_call_root` does. The frame opens the executor frame once (`open_resolve_call_build`) and steps the walk (`step_resolve_call_build`). Its walk (`run_resolve_call` → `prepare_call_plan` + `walk_call_plan`) acquires the candidates and arguments once (`CallPlan`). Before each first-pass argument relation, the walk the frame is stepping (`call_walk_suspends`) demands what that relation unwraps from the argument's and the parameter's types (`call_operands_wait`, at the decision seam the frame installs for it): an answer its frame holds, or a warm result, reads at once; one that must be computed is a need. The walk then stops (`CandidateVerdict::Suspended`, `CallSuspension`) with its candidate's collecting session taken OUT of the transaction (`take_collecting_session`, the same hand-off the contextual route uses), so nothing its needs evaluate — a call nested in them included — deposits into it. Once every need is delivered the walk resumes at that candidate and that argument (`take_call_resume`) with the SAME session restored, its checkpoint, failure, SCC-deferral flag and skipped generic-function arguments; it never repeats the receiver, generic-rest or earlier argument relations. A frame abandoned while suspended returns its collector to the transaction abandoned and pops its executor frame (`abandon_resolve_call_build`), so no session that may collect and no obligation frame outlives it. Error-recovery candidates, the contextual route and every call evaluated in place still relate in place. `semantic_task_tests` suspend `pick(1, box)` with `T`'s first deposit held, resume it on another thread to the uninterrupted `1 | 2`, and release or drop it while suspended with nothing left open.
- A function's return demanded as a machinery root outside any drive (`drives_on_the_runtime`: `FlowReturn` while `decides_root`, when its body calls with a function-value argument, read off its indexed call sites — `flow_body_types_callbacks`; the one flow executor `execute_flow_return_cold_build` drives it, and any other root evaluates synchronously with each read driven where it is made) is a frame too (`FrameBuild::Flow`, `FlowBuild`, `drain_flow`), through the same family claim, admission and publication; its admitted publication drains the members its evaluation completed (`flow_return_drain_completed_members`), and a missing one aborts them. A step runs the function's evaluation (`build_flow_return`) under the frame's flow seam (`FlowSeam`), as a conditional decision runs: an instantiation or `typeof` the function's OWN body reads at query level (`nested == 0`, at the root's own transaction frame depth — never a query evaluated in place, never a callee evaluated inline beneath it) is answered by the frame's deliveries, served warm, or becomes the frame's need and reads as a typed gap, and a run that reached a need is dropped with what it observed (`observe_read` + `discard`) and runs again once each is delivered, so a chain of values the body reads is a chain of frames, as the synchronous build's drive of each read made it. A contextual window is open while the contextual route types a callback the executor's second pass names (`RouteAt::Finishing`, its candidate's collector held out of the transaction in `ContextualCallState`), and while the typed callback's relation to the held target is pre-demanded (`contextual_delivery_waits`, the operands plus a signature's parameter and return types, as `call_operands_wait` does for a first-pass argument); it opens only while the run has no need yet. A run that reached a need inside a window holds the call's route as it stood BEFORE the callback (`HeldContextualRoute` around the `FinishRoute`, collector, step observations and recorded undecided relations included), takes the call undecided, and is dropped; once each need is delivered the next run reaches the same call with the same key and `ask_resolve_call` resumes the held route instead of asking the executor (`take_held_contextual_route`, `resume_held_route`: this run's argument observations, the held route's undecided relations counted again), so the callback types over answered reads into the SAME collector. A frame abandoned between runs releases every held route — observations discarded, its collector returned to the transaction abandoned (`release_contextual_collector`) — and aborts the members its runs completed; a completed run releases any route it did not reach again. A finished contextual candidate's collector also returns to the transaction abandoned rather than being dropped outside it. `semantic_task_tests` suspend `both(1, (x) => late)` with `T`'s deposit held, resume it on another thread through the held route to the uninterrupted `[number, Box<3>]`, and release or drop it while suspended with the collector released and nothing left open.
- A task carries the cancellation and accounting its lender's thread installed: each poll installs the scheduler job token current when it was lent (`JobCancellationGuard::replace`, clearing an unrelated destination job), so an aggregate job's live requester keeps a task alive on any thread; and the task's own component-meta span, request spend log and refusal scope (`TaskSpanCarry`, `TaskSpendCarry`, `TaskRefusalCarry`, `TaskCompletenessCarry`), standing for whichever of them were open where it was lent, plus its cold-compute completeness and sticky-partial deferral depth, so a partial fold cannot taint the polling thread or bypass the lender’s sticky deferral, and a charge made on another thread never lands in that thread's unrelated computes and is folded into the lending compute's on return. A task dropped instead of returned leaves its request state, construction owner, abort and accounting in its lender's `home` slot (`UnreturnedTask`); the lender's `LentTask` reclaims the state before releasing that owner on the lender's thread, so its construction scope absorbs the roots. Dropping the task on another thread never releases those roots there.
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
- Overlap between a cold producer and a subscriber is tested by construction,
  never by arrival timing: `SemanticGraphStore::arm_flight_rendezvous(key)`
  (compiled only under `test`, `test-support` or the default-off
  `soak-support`, which `verter_session/soak-support` forwards to the
  soak/tooling runner) parks the first joinable producer of `key` on that
  store at the dispatcher's synchronous cold-build boundary — flight claimed,
  raw build not begun, no memo or flight lock held — reports a subscriber
  only once its wait-for edge registered on that exact flight, and releases
  the producer on `release()`, when its own request is cancelled
  (`cancel_participants()` cancels the parked producer and the registered
  subscribers), or when the guard drops (completion, unwind, abandoned
  schedule). A guard's drop removes exactly its own registration, so
  overlapping scopes may end in any order. Other keys, stores and warm
  reads never satisfy it; an `Instantiate` key driven outside any drive
  runs on the continuation runtime and never reaches the synchronous
  boundary, so arming one is refused. Inventory:
  `crates/verter_type_engine/observe-inventory/SKR-FLIGHT-RENDEZVOUS.md`.

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


A lazy declared call return is checked at its expression site before a later
consumer can choose a type site. Indexed initializers await the return's
instantiation (`IndexedNext::AwaitReturn`). A flow body checks it as an
operand of its own flow frame, or as a drive of its own outside any drive.
Anywhere else inside a running drive — a body an inline signature probe
evaluates beneath a decision, a callee evaluated inline — the check is not
made and the call keeps its lazy carrier: starting the return's work there
would nest natively, one level per declared return a chain of calls reaches.
A typeof frame installs no decision seam: its projection consumes each
delivery where its evaluation reads it, so a sequential chain of reads in
its body (an alias walk, say) costs one step per read rather than a full
re-run per read. A probe beneath a decision with a pending operand prevents
its tentative source shape from warming. Only a checker recovery replaces a call's carrier, with its
expression diagnostic; any other read keeps the named carrier. These reads
never drive another execution inside a frame step.

A semantic task's demand identity is the query together with its checking
site (`QueryDemand`): a result completed at one site never answers a need at
the other, and a need closes a cycle only through an open demand at its own
site.
