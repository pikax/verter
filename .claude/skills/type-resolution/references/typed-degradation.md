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

A successful, complete lookup may return a typed negative carrier for a proven
absent member or stable unresolved authored reference. Settlement preserves that
exact carrier and its reuse evidence. A failed query supplies no value; an
incomplete lookup cannot establish absence. A completed positional value may
retain the unresolved position as an approximate gap, while a failed lookup or
presence-only subset remains Unavailable. No gap establishes an exact negative.
An unmodelled value position has the distinct `UnsupportedPosition` disposition:
it cannot prove absence or publish an exact standalone type. Its enclosing
completed structural producer may preserve it as a positional typed gap.

The producer must establish the usable interpretation, independently of cause
codes and incidental markers elsewhere in a value:

- Shallow member/key enumeration returns proven presence subsets. Missing
  contributions prove neither absence nor exact member values/modifiers.
- Completed positional lowering retains unresolved authored carriers at their
  demanded positions. Unrelated siblings keep their independently evaluated
  types. A short structural walk cannot claim this guarantee.
- A typed undecided conditional preserves its operands and branch carriers as
  a positional gap; only exact conditional readings can select a branch.
- A stopped connected demand is Unavailable. Its addressed source may remain
  in internal diagnostic state, but an unevaluated continuation establishes no
  positional guarantee and is never offered as a usable value.
- A root `CheckerRecovery` outcome carries the checker's explicit recovery and
  diagnostic, including TS2589/TS2590; an arbitrary budget reason does not.
  Reprojecting that root preserves its typed basis: `Budget` remains
  `Approximate(CheckerRecovery)` with its operation-budget cause and is never
  admitted, even when the new walk observed no additional limitation.
  `Certified` remains exact under a complete read. Carrier normalization and
  merged-global declaration delegation retain the adopted interpretation when
  joining their own causes; an anchor change cannot withhold a usable surface.
- A found declaration placeholder is an exact expandable source, not a failed
  normalization. Its shared path walker still owns the demanded instantiation.
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
only `Complete`. `FactStatus::Approximate` carries the producer's
`FactInterpretation` through cold delivery, retained reads and singleflight
followers. Forwarders preserve it; distribution may construct a positional
representation only when every adopted arm has completed positional structure
or explicit checker recovery. A presence-only child cannot establish that.
Discarded probes contribute no quality to an independently
derived answer; adopted incomplete dependencies degrade it. Execution validity
is non-discardable and supersedes every approximation.

An abandoned read-observation scope restores every quality rail, including its
degraded-member counter, on normal drop and unwind. Finishing the scope captures
the private counter delta before restoration. Missing graph nodes supply no
positional source: root and child projection failures are unavailable, never
inserted into the exact projection memo. Repeated visits retain that disposition.
Composite absorption, exhaustive union distribution, template constraint
capture and flow key-presence/absence proofs also require complete operands.
A producer's explicit root checker recovery may propagate unchanged through
these value reductions; it cannot establish a branch, modifier or absence.
Usable positional values remain available to ordinary value consumers.
Conditional decisions retain the completeness of operand normalization before
relating either operand. Relation early rejection cannot establish `False`
when that normalization lacked exact evidence, just as approximation cannot
establish `True`.
The relation's own observed dependencies must also be complete before a
verdict selects a branch: nested member values may first be demanded after
operand normalization. An incomplete relation retains the conditional shell
and its adopted causes, rather than committing an approximate winner.
The relation producer grants checker recovery only at its decided root close,
when the payload is false and the typed comparison refusal names that exact
operand pair. Inherited diagnostics cannot authorize a different verdict, and
a mixed close with unproven flow evidence supplies no relation interpretation.
The exact pair's refusal may supply the checker's explicit false recovery. The conditional propagates
the authored false value as Approximate with the refusal diagnostic, without
publishing a `ConditionalSelect` proof or counting an exact branch decision.

Tuple operand construction can retain a checker recovery only after every
position normalizes through the shared demand to an exact answer or its own
typed root recovery. The resulting checker-selected value is Approximate;
it emits no branch-selection proof. An unsupported partial position cannot
supply that guarantee.

### Signature source and selected member evidence

Expanded signature projection demands a deferred body return in its declaration
environment and applies the captured lexical images once. Shallower projection
keeps the source without demanding its value. A signature with an absent return
source has no owned body to defer: strict source publication must still reject
its interior miss.

Signature construction owns a return source. Its shared optional eager probe
retains a value only when the captured value observations are complete and
the payload is complete or supplies a separately certified exact source value.
A declined probe constructs the opaque `DeferredFlowReturnSource`
token and `SignatureReturnCarrier::DeferredFlow` inside the sealed
`DeferredCallable` header. That type has no return-value slot, so direct
signature readers cannot mistake a deliberately deferred value for a real miss.
The shared value-demand accessor turns it into a general signature only after
consuming the owned source. Header-only utilities read its parameters without
forcing the return. Values retained by signature construction use `Function(Flow)` and
remain visible to unresolved-value checking. Return demands, call substitution,
signature result recipes and source roots use the same existing flow source.
After a value demand, `SignatureReturnCarrier::EvaluatedFlow` retains the
same source and lexical images beside the evaluated slot. A signature utility
result demand receiving only that node re-reads the owned flow source to adopt
its actual fact in the utility's scope; the stored slot alone cannot prove
exactness. Header-only utilities continue reading parameters without forcing
a body. Call equations apply the source images once to their newly inferred
body result. The deferred state survives signature substitution and
structural rebuilding.
It retains sorted lexical clause names and the header images supplied by the
actual lowering environment, excluding the signature's own clause parameters.
The call resolver instantiates those parameters through the body-derived carrier.
Signature substitution,
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
and the closed nameless primitive/literal/Union source algebra. Unconditional
block wrappers around a closed return preserve its eligibility; the source walk
admits only straight-line bindings followed by one return or nested block.
Successful product finalization is required even without a prepared flow plan.
The existing
evaluator must actually complete its lowering, joins and widening with complete
value observations, no hold or fallthrough, and valid execution. The certificate
binds the function, demand, produced node and observed source roots. Named or
unsupported expressions, later spreads/computed overrides and truncated walks
cannot certify. Consuming it retains roots and reuse suppression; it never
promotes the original `FlowReturnResult::fact()` or its admission proof.
The flow producer offers its selected value fact separately from whole-proof
refusal. Adopting that value retains every observation of its value read,
including partiality, degraded reads, roots and reuse suppression. A required
value limitation withdraws an earlier certificate; refusal of the separate
whole-function proof cannot change the certified value's fact.
The producer's completed literal-widening operation transfers an existing
certificate to its widened node. Arbitrary result replacement still withdraws
the certificate; widening cannot mint one for an uncertified source.

Nested constructive evidence also requires successful product finalization.
A missing product degrades the nested value and cannot supply a certificate to
its enclosing evaluator, even when the return's authored type source is closed.

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
binder with a completed closed bound source. A non-generic, parameter-free
plain method header can compose only after its nested producer finishes its
products and independently certifies the actual constructed return or completes
the existing nameless primitive/literal/Union/finite-array return grammar.
That evidence binds
the served function and produced signature; each new attempt withdraws its
previous certificate. Equal signature nodes from another body supply no proof.
Unknown, computed, held and unmodeled positions cannot seal this evidence. Sibling bound/default
source capabilities compose transitively: a served name alone cannot certify
a binder whose referenced sibling is unsupported. Each authored bound/default
is inspected once; indexed reverse dependencies propagate refusal without
repeated clause scans. Mutually closed sibling sources keep their capabilities.
The existing call executor
supplies a result witness only from an actually completed selected
declared recipe; a provisional seed is insufficient. Its actual substituted
constructor and result must match the sealed class source. Closed tuples
compose only certified elements; holes and spreads do not certify. A completed
unconditional return can consume this separate type evidence. A selected local
class binding's exact served declaration identity must match the actual
producer-certified constructor to supply binding source evidence. This proves
its finite type source, never the class's runtime capture set: the full Capture
obligation remains an accepted typed gap and the whole frame never admits warm.
Class expressions carry no declaration identity. Unsupported class members,
effects and uncorrelated captures retain their required observations.

A finite object data literal can compose a type certificate only when every
actual data value completed, static keys form its whole surface, and no spread,
method, recursion, withheld inference, hold, degradation or incomplete read was
adopted. The enclosing completed walk and binding products certify a selected
return value separately from runtime capture completion. Selected value readers
use `execute_flow_return_value_fact` through the same source producer; a whole
flow-frame observer still sees its capture refusal. Roots, reuse suppression and
execution aborts survive that independent value demand.

A body-degradation read adopts its observations only when the callee's actual
degradation is copied. A declined diagnostic probe cannot contaminate the
separately adopted return-value source; its reuse refusal, roots and execution
aborts still propagate.

A direct call may decline an undecided executor when its declared return is
independent of the callee's clause. That fixed result has its own producer;
the declined executor's applicability observations and exact undecided-relation
delta do not belong to its proof. Authored argument observations remain adopted.
Retyping similarly discards the replaced executor attempt before adopting the
new one. Execution aborts propagate through both adoption and discard; an
argument-dependent, retyped or unsupported result receives no exemption.

Transient indexed call arguments attach each callback's exact served return
source by canonical program point and matching authored signature span before
signature lowering. Both expression and flow-call demands use the same source
linker. Inferred body-return spellings then use the existing flow producer
rather than resolving callback-local names in file scope. Authored TS return
annotations and parameter reads keep their own required evidence.

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

A shallow result-utility surface demand
consumes the producer's certified static return keys; each member keeps its
separate deferred indexed value source. Presence alone cannot prove key-domain
closure. A required output reduction that returns `IncompleteSemanticOperand`
retains that typed failure; it cannot republish its authored subject as a
deliberately unreduced carrier.

Explicit signature instantiation records consumed own-clause images on its
deferred result source. Uninstantiated signature construction captures lexical
outer images only. The later result demand applies those images once, including
class capture environments. Getter reads demand that result; setter reads need
only the sealed parameter header. Heritage's own-member shortcut uses the same
member reader as ordinary projection, preserving accessors, overloads and
visibility without observing unrelated base values.

A required call retains its adopted no-surface causes even when the enclosing
value represents that call as a positional gap. Independently certified sibling
values keep their separate observations. A qualified `typeof` demand uses the
shared root-and-path lookup; a failed member lookup cannot retry a joined name
that bypasses the namespace's excluded-export fact.


A completed path walk may forward a child's Positional interpretation only
when it returns that adopted child's actual node and every incomplete child
supplied its own positional guarantee. Local stops, unavailable dependencies,
surface failures and open spreads cannot mint this forwarding guarantee.
Variance marker comparisons are optional proof probes: only complete binary
comparisons contribute measured variance; declined or incomplete measurements
leave the original structural comparison responsible for the answer.
Macro carrier-discrimination probes preserve their walker diagnostics even
when no surface value can be published. Their availability and diagnostic
channels are independent.
Full metadata normalization preserves every named row when its required member
source is unavailable, while that typed source-construction failure degrades
the enclosing metadata DTO and prevents warm admission. An exact key domain
does not certify all of its required value positions. A shallow output
vocabulary limit alone does not establish such a semantic failure.

Runtime classification consumes Complete subject facts. A macro's incomplete
whole payload is an optional probe when its original source independently
certifies the demanded member: that Complete member fact replaces the declined
payload's quality, while aborts remain authoritative. No flow-reason allowlist
authorizes classification or prevents the independent member demand.

Nested carrier and deferred-evaluator fact finalization treats a live-domain
mutation as `Superseded`, including direct reads without a request scope. A
declined quality probe cannot discard this execution failure. Direct cold and
warm structural demands both check the active abort before serving a value;
a cached independent source cannot bypass execution validity.

The sealed resolved-surface token carries Svelte's captured props body into
shared member-source construction. Required source validation precedes the
member-path publication for both adapters. A failed source is never temporarily
published and repaired after it has contaminated the enclosing fact. Inline
callback signatures retain their exact source; named callback aliases stay
shallow. Bindable rows use the same captured body.
