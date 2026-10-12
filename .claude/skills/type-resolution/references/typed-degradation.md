## Typed Degradation And Completeness Contract (CRITICAL)

Semantic degraded states are part of the type system contract. They must be typed, propagated, and observable.

- `TypeExpr::Unknown(UnknownValue)` is not a carrier for semantic control flow. Misses, unsupported intrinsics/operators, alias cycles, recursion sentinels, budget exits, unstable-state exits, and bridge-depth exits must use typed variants or typed sidecar state. Do not encode them as strings such as `"budgetExceeded(...)"` / `"aliasCycle(...)"` and do not recover them with `starts_with` / regex checks.
- `Unknown` is allowed only for a genuine unknown type value with provenance explaining why the producer could not represent it. If the producer knows this is `Unsupported`, `BudgetExceeded`, `Recursive`, `Miss`, or `Unstable`, use that state instead.
- Public query envelopes must preserve completeness. `Complete` means the required inputs were available, current, and no budget/unsupported/unstable branch affected the answer. A query may return `Complete(None)` only when absence itself was proven under the current facts; missing analysis, stale cache data, unavailable providers, unsupported operators, and budget exits must surface as `Unavailable`, `Partial`, or a typed degraded result.
- Degraded results may be displayed and returned to callers, but must not be promoted into warm shared caches as complete answers.

Guards: `macro_impacting_constructs_fail_lowering_not_silent_skip` (`crates/verter_session/tests/cases/architecture_guards.rs` + `crates/verter_session/src/owned_artifacts/eval_program_tests.rs`) pins fail-loud lowering over silent skip; `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries` (`crates/verter_session/src/component_meta_audit/mod_tests.rs`) pins published-field provenance at the macro boundaries. Typed degraded-state propagation beyond those two boundaries is not yet guarded end to end.


### Producer fact guarantees

Producers state their answer's quality as a `FactResult` (`semantic_query/fact_result.rs`) derived from their own typed outcome; the arm decides, the causes explain.

- **Flow return** (`project_semantic_dispatch/flow_return_fact.rs`): an undegraded evaluated return over a read that observed no partiality is `Complete`. A degraded success (`FlowReturnDegradation`), or an undegraded value whose read observed partiality, is `Approximate` — the documented usable interpretation is the evaluated return itself, with exact siblings around a positional marker (`FlowReturnUninferred`), a complete member set whose types may be unverified (`FlowReturnUnverified`, which is where an unapplied write lands), or the checker's budget recovery (`OperationBudget`). A typed `FlowReturnFailure` or a surfacing hold is `Unavailable` with `FlowReturnNoSurface`. A read that observed cancellation or a superseded/torn view is an `ExecutionAbort`, not a fact.
- **Scoped semantic probes**: a read whose consumer may decline it runs under `ProjectSemanticDispatch::observe_read`, and the consumer states `adopt` (its observations join the enclosing build) or `discard` (a fall-through route owns the answer). A declined probe never marks its enclosing build partial; an adopted one never hides its partiality. Signature lowering owns the return source carrier and can decline an eager body probe; `ReturnType` demands that source when its value is required. Closed `Pick` demands the source's shallow key/member structure rather than expanding unrelated member values.
- **Conditional reading** (`project_semantic_dispatch/conditional_decision.rs`): `NotConditional`, `Reduced` and a conditional the checker keeps (`Deferred`) are all `Complete`. A conditional the query could not finish is `Unavailable` with `UndecidedConditional` plus the read's own classes, its error's classification, or same-path recursion; a partial read's value is never offered as an approximate reading. Consumers relate only exact readings.
- **Surfaces**: `SurfaceResolution::into_fact` maps a closed surface, an open presence-only domain and `NoSurface` to `Complete`; an incomplete claim with a usable subset is an approximate presence-only domain, without one it is unavailable.

### Flow execution validity

Flow execution uses `ExecutionAbort` independently of semantic completeness.
`ExecutionValidityScope` in `request_context` is compact production state, not
an optional observation history. Originating cancellation and rejected evaluation
provenance record the typed abort; nested scopes always propagate it, including
when `observe_read` discards an optional proof probe's completeness.
Fresh top-level executions start clean. `QueryError::ExecutionAborted` carries
the disposition through the memo's producer settlement and follower delivery.
Scoped flow observations retain the abort independently of their quality;
both `adopt` and `discard` return `Result<T, ExecutionAbort>`, so declining
quality cannot extract a payload after execution failed. The scope guard is
unforgeable and thread-bound; it cannot be moved to pop another thread's state.
Settlement refuses admission and replaces any value after an observed abort.
Mutation-unstable evaluated reads and invalidated producer flights deliver
`Superseded`, never a retained value. Fenced serves and unrootable admission
carriers alone are reuse refusals, not evidence that evaluation was invalid.

The consuming boundaries are generic dispatch, `execute_flow_return`,
`execute_function_return_source`, and the audited host flow API. Their abort arms
carry no value. `FlowReturnResult::fact()` exposes Complete or Approximate on
valid evaluated values and preserves quality across node rebuilding. A typed
failure or surfacing recursive hold is Unavailable. Compatibility cause sets
explain these dispositions and cannot determine execution validity.

| Refusal | Evidence and disposition |
| --- | --- |
| Missing preparation / unserved preparation site | Evaluation subsequently served its own source and completed its structural walk. Its evaluated return is Approximate, with the preparation cause, and never admitted. |
| Preparation budget / unplannable operation | Same independent evaluation requirement; Approximate is limited to that evaluated return, with unresolved positions preserved. An evaluation that itself fails supplies no value. |
| Stale basis / foreign evaluation or demand provenance | The value/report no longer binds to the installed current demand. Superseded abort, no value and no shared candidate. |
| Lost installed demand / mismatched obligation set | Completion evidence cannot establish the value's required binding. Unavailable, not a usable approximation. |
| Cancellation / shutdown | Typed execution abort, irrespective of any apparent value or cause summary. |
| Panic / internal typed failure | No trusted evaluated result; Unavailable. A typed stale failure is Superseded. |
| Incomplete obligations / unobserved convergence / typed gap / result-contract refusal | Completed independently sourced evaluation remains Approximate; its return is observable without claiming a completion proof. |
| Short or aborted evaluation walk | No independently completed evaluation: Unavailable. |

`UNSTABLE_STATE` may still explain refused preparation. Flow consumers never
convert it into an abort. Non-flow projections use the same typed abort
channel, independently of their fact status. The `FlowReturn` key supplies no delivery
exemption. Optional derivation histories continue to follow `semantic-observe`;
the mandatory validity disposition and producer quality are retained regardless
of tracing configuration.

### Projection delivery and approximation

Memo settlement adopts the producer's `FactResult` before broadcasting to
the owner or singleflight followers. `CacheRead.fact_status` and public query
provenance preserve that disposition; `result_is_partial()` and cause summaries
derive from it. Retained complete repeats carry `Complete`. An unavailable
fact delivers `IncompleteSemanticOperand` without a usable candidate, and
neither approximate nor unavailable facts enter the family memo.

The producer must establish the usable interpretation, independently of cause
codes and incidental markers elsewhere in a value:

- Shallow member/key enumeration returns proven presence subsets. Missing
  contributions prove neither absence nor exact member values/modifiers.
- Completed positional lowering retains unresolved authored carriers at their
  demanded positions. Unrelated siblings keep their independently evaluated
  types. A short structural walk cannot claim this guarantee.
- A typed undecided conditional preserves its operands and branch carriers as
  a positional gap; only exact conditional readings can select a branch.
- A connected-demand refusal can retain the addressed type's source and operands
  as a suspended continuation at that position, never as its evaluated answer.
  Unsupported demands with no such carrier are unavailable.
- A root `CheckerRecovery` outcome carries the checker's explicit recovery and
  diagnostic, including TS2589/TS2590; an arbitrary budget reason does not.
- An independently evaluated flow return retains the guarantee described above,
  even when preparation refused its proof. Cancellation or invalid provenance
  still takes the abort channel and supplies no value.
- A completed call result retains the evaluated return positions when its
  enclosing flow component refuses its proof. Its selected signature and
  substitution remain provisional evidence: the adopted call fact is
  Approximate, supplies no exact applicability judgment and cannot be admitted.
  A scalar relation computed from the same refused evidence has no positional
  interpretation and is Unavailable.

Conditional selection, runtime classification and other proof consumers accept
only `Complete`. Discarded probes contribute no quality to an independently
derived answer; adopted incomplete dependencies degrade it. Execution validity
is non-discardable and supersedes every approximation.
Composite absorption, exhaustive union distribution, template constraint
capture and flow key-presence/absence proofs also require complete operands.
A producer's explicit root checker recovery may propagate unchanged through
these value reductions; it cannot establish a branch, modifier or absence.
Usable positional values remain available to ordinary value consumers.
Conditional decisions retain the completeness of operand normalization before
relating either operand. Relation early rejection cannot establish `False`
when that normalization lacked exact evidence, just as approximation cannot
establish `True`.

### Signature source and selected member evidence

Signature construction owns a return source. Its shared optional eager probe
retains a value only when the captured value observations are complete and
the payload is complete or supplies a separately certified exact source value.
A declined probe constructs the opaque `DeferredFlowReturnSource`
token and `SignatureReturnCarrier::DeferredFlow`; its provisional return slot
is not an evaluated gap. Values retained by signature construction use `Function(Flow)` and
remain visible to unresolved-value checking. Return demands, call substitution,
signature result recipes and source roots use the same existing flow source.
The deferred state survives signature substitution and structural rebuilding.
It retains sorted lexical clause names and the header images supplied by the
actual lowering environment, including its own clause. Signature substitution,
binder collection and stable encoding carry those images. The existing flow
binder rebinder applies them after declaration-body inference; nested qualified
binders remain separate. This preserves overload selection in the declaration
environment while specializing the inferred result at its consumer.
While the owning class constructor header is being composed, its own member
body probes defer at that served source anchor. This avoids demanding a body
that can depend on the same unfinished header. Its lexical binding images and
receiver survive until the ordinary value demand; no return value or admission
proof is inferred from the deferred source.
Constructor-object composition consumes the shared located header shape and
the existing narrow-binder substitution directly. It retains static value
source carriers; the selected member demand owns their later evaluation.
Whole member projection cannot precede that structural constructor demand.
An `Expanded` constructor demand projects only its declared construct-return
positions through the shared carrier resolver. Other demands retain named
instance sources, and sibling static value sources remain deferred.

A body-position signature utility with a closed function/object argument
demands its return source at that position. It does not publish another
`ReturnType` carrier over an already resolved body return. This preserves
linear work for type-position chains and exact source values alongside a
refused public flow proof. Explicit call type arguments lower under the
caller's lexical binder environment before declaration-body inference.

Returning a named callable value demands its deferred call-signature return
slots, including callable object entries. The value producer rebuilds those
slots with evaluated returns or typed gaps and inferred predicates; merely
carrying that signature as a type operand does not demand its body. Only the
dormant deferred placeholder is excluded from unresolved-value traversal.
Actual evaluated slot values and gaps remain observable.

Both whole mapped-surface producers and selected-key projections apply the
same optional-value transformation before discarding source modifiers.
Removing optionality strips its added `undefined` under `strictNullChecks`;
an originally required member retains an explicitly authored `undefined`.
With `exactOptionalPropertyTypes`, the optional flag carries absence and
removing it preserves explicitly authored `undefined` in the value slot.
Whole surfaces keep added absence on the optional flag. A member read owns
the corresponding `undefined`; source-slot publication does not add it to
the stored value.

Required source-slot inspection uses the existing shape fold in its private
source-presence mode: an owned body return or schema-proven absent return slot
does not make the signature source unavailable. Parameters, constraints,
defaults and predicate slots still participate. This source fact supplies no
return value; ordinary value/domain shape inspection retains its full checks.

One static member may establish separate exact value evidence while the whole
flow-return proof remains refused. The private producer certificate is limited
to a single unconditional object return, a completed selected static-key path,
and the closed nameless primitive/literal/Union source algebra. The existing
evaluator must actually complete its lowering, joins and widening with complete
value observations, no hold or fallthrough, and valid execution. The certificate
binds the function, demand, produced node and observed source roots. Named or
unsupported expressions, later spreads/computed overrides and truncated walks
cannot certify. Consuming it retains roots and reuse suppression; it never
promotes the original `FlowReturnResult::fact()` or its admission proof.

A whole return can separately certify closed primitive/literal operands,
their unions, and finite arrays without spreads or holes. Every consumed
return contribution must carry that source eligibility, and the actual
evaluator must complete its selected walk and product ledger with complete
value observations, no holds, no degradation and valid execution. This works
through supported blocks and control flow without changing proof-resource
limits. It never certifies a named read merely because its interned node equals
a previously certified literal. The public flow disposition remains separate.

Deferred evaluation preserves adopted availability across value demands.
Its usable-node adapter carries bounded approximations; exact structural
classification still rejects them. A stopped instantiation or template
construction retains all original application or interpolation operands as a
suspended positional source. A limit with no addressed source has no value.
An honest negative declaration probe can retain its independently authored
carrier; an incomplete lookup has a distinct typed error and cannot take that
discard route.

Class composition can separately seal its finite type source when every
authored key, field operand, constructor parameter and heritage source has
positive typed construction evidence. The bounded operand vocabulary is
primitive/literal/Union syntax or a reference to an actually served clause
binder with a completed closed bound source. Unknown, computed, method-body,
held and unmodeled positions cannot seal this evidence. Sibling bound/default
source capabilities compose transitively: a served name alone cannot certify
a binder whose referenced sibling is unsupported. The existing call executor
supplies a result witness only from an actually completed selected
declared recipe; a provisional seed is insufficient. Its actual substituted
constructor and result must match the sealed class source. Closed tuples
compose only certified elements; holes and spreads do not certify. A completed
unconditional return can consume this separate type evidence while the full
runtime capture proof remains refused. Roots and reuse suppression survive.

Forward receiver reads in object and class methods hold each stepped attempt's
observations separately. A replaced first attempt discards its observations;
the retained attempt adopts them. Execution aborts remain non-discardable.
Non-expanded object navigation preserves member value sources directly, without
inserting them into the ordinary projection memo. Computed keys and signatures
still demand their structure; an addressed value or expanded demand forces the
source through the same existing projection pipeline.

Shallow mapped enumeration over the authored `keyof` of its source may consume
a typed incomplete presence subset without proving the key domain closed.
It carries only proven source members, preserves source causes and modifiers
as incomplete evidence, and supplies no absence or exact modifier/value proof.
Arbitrary keyspaces and remaps still require the existing exact key-domain
authority. Complete generic deferred mappings retain their exact disposition.

Normalization cause joining preserves an already adopted bounded approximation
and typed errors/aborts. A complete warm value under an incomplete normalized
basis has no automatic approximation guarantee and is withheld.

An outgoing provisional flow value is approximate until its owning component
closes. The pending ledger retains the actual evaluated operands for that
closure. A call poll that records a hold discards the provisional value and
its observations; a retained closed seed adopts them. Optional freshness
proofs require complete payload and observations and discard declined reads.
This prevents a signature source probe from mistaking a provisional seed for
an independently completed return while preserving exact coinductive closure.

Every root and relationship close stamps its actual typed proof refusal before
recording the transaction-local value. The closed-value channel carries the
result, finalized proof, source roots and separate reuse suppression. An
outside-component return equation adopts those facts before extracting a node;
its in-component provisional override remains owned by the current equation.
Benign suppression on a proven complete value remains separate from semantic
partiality. Native flow replay and instantiation reads adopt the carried fact
status even when a receipt alone would only suppress reuse.
