# Receiving fact guarantees

This is the receiving contract for TypeInfo-first component views (TIF),
native diagnostic families (NCK), authored language-service operations (LSO),
and their generated semantic slices. It specifies what these consumers must
preserve when adopting the shared engine's facts. It does not grant a new
cache family to an incomplete formatter, provider, index, glob, or projection.

## Demands and authority

Each consumer declares the question it needs and reads the corresponding
`FactResult` with its dependency evidence. Membership, one member's value,
an authored splice, synthesized emits, testing rows, and a root-narrowing
claim are separate demands. Work that was never demanded contributes no
status. A file or product summary is derived from the statuses of the facts
that product actually publishes; the summary never authorizes a fact.

| Demand | Complete guarantee | Incomplete interpretation |
| --- | --- | --- |
| Membership | A closed domain proves absence as well as presence; an open domain proves only its published positive members | An approximate domain is a lower-bound enumeration; omitted members prove nothing |
| Member value or runtime constructors | The producer established this member's own value or classification | An unavailable value cannot become TypeScript `unknown`, a runtime validator, or an exact sibling's status |
| Authored splice | The requested authored syntax and its bindings are faithfully preserved | A semantic value refusal is independent of this guarantee; missing authored syntax is a splice refusal |
| Synthesized emits or testing rows | Every published row has the specific complete facts its derivation requires | A row requiring an unavailable value stays refused; a separately proven row remains usable |
| Root narrowing, exclusion, or exhaustive negative claim | Exact coverage and all value/effect facts required by the inference are complete | A presence-only subset, unavailable input, or cached refusal cannot prove the claim |

`FactResult::and_then` and `zip` express dependent composition. Independence
needs the producer's rule and exact coverage of the independent demand; it
cannot be obtained by subtracting a cause, clearing partiality, inspecting
an enclosing request flag, or declaring that a sibling was complete.
The value and status travel together through cold evaluation, request memos,
warm delivery, and singleflight joins. Causes explain an arm and never select
a more permissive arm.

## Reuse and publication

Reuse preserves the complete answer or the original refusal, including its
status and recovery. The receiving boundary must distinguish these cases:

| Admission class | Required identity and evidence | Delivery |
| --- | --- | --- |
| Complete fact | Exact demand and context, complete coverage, current dependencies, and a valid publication fence | The complete fact |
| Certified divergence | The producer's deterministic divergence certificate for this demand and context, with its observed dependency prefix | The original certified outcome and recovery |
| Operation-profile refusal | The exact operation demand, context, and operation profile, with the refusal's observed dependencies | The original refusal and recovery; a higher allowance misses and recomputes |
| Isolated-root ledger refusal | The same isolated root, profile, entry allowance, and paid-set/context conditions, with the recorded charges and dependency prefix | The original refusal with its accounting replayed |
| Other approximation or unavailability | No retention authority | Return to this caller only; missing inputs can heal on the next demand |
| Execution abort | Cancellation, shutdown, or a superseded/torn view | No published fact or retained refusal |

The three refusal classes remain refusals. Their recovery never turns the
refused fact into a successful value or authorizes a dependent exact claim.
A retained child refusal does not admit a partial wrapper. A product needs
its own deterministic demand and exhaustive admission decision; receiving a
refusal as an ordinary successful value is forbidden. Changing the root,
context, profile, entry allowance, or consulted dependency invalidates the
corresponding reuse authority. Retention remains bounded by the owning store
and the shared retention account.

Adopting a read adopts its dependencies even when an independent producer
rule establishes a complete sibling. Validate a warm candidate before
replaying its signature into every enclosing tracer. Cold publication uses
the engine-owned finalized trace, exact owner coverage, and the request's
captured publication fence. A rejected read contributes no candidate facts.
Final component consumers use the session facade over the engine's narrow
read, trace, and admit operations; they never inspect a production raw map
or substitute an untraced signature.

## Receiving boundaries and proof

TIF component views preserve each facet's demand and status. NCK diagnostic
families require the complete facts their diagnostic or narrowing rule uses;
an unavailable fact cannot prove that a diagnostic is absent. LSO operations
preserve authored-coordinate and semantic guarantees separately; a usable
authored target cannot authorize an unavailable type or edit precondition.
Generated slices declare these same demands and admission cases rather than
generating a blanket Complete-only policy or a cause allowlist.

Each receiving product proves its adopted boundary with an unavailable
member, an exact sibling, and an independently complete authored splice.
It also proves refusal repeats with zero semantic producer executions,
higher-budget healing, relevant-edit invalidation, root/context/allowance
isolation, missing-input healing, and fail-closed runtime and narrowing
controls. Compact guarantees and adopted-dependency propagation are required
production state. Historical counters and cause traces are optional and
compile out without `semantic-observe`; test-support observation seams do
not turn that feature on in production.
