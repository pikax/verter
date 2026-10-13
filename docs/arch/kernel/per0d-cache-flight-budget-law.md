# Cache, flight, cancellation and budget law

This decision fixes the reuse and performance law consumed by every successor
capability. The current consumers have local caches, coalescers and budgets.
The final authority is the accepted identity, mapping and shared cache runtime,
with the per-operation budget and publication law below. Mechanisms remain
SKR-owned; this decision creates no runtime, store, key type, counter or gate.

It describes the repository at `docs(arch): define atomic contribution and
candidate contracts (#879)`, 2026-10-13. That title and date are descriptive
context, not proof. It follows the [docs-only rule](README.md). This file holds
the law, consumer inventory and receiving case table; there is no additional
schema, registry or test product.

The inputs are the [identity law](identities.md),
[coordinate law](coordinates.md), [parser law](parser-ownership.md),
[demand plan](demand-activation.md), [TypeInfo contract](typeinfo-facade.md),
[ComponentInfo view](tif1d-componentinfo-view.md),
[contribution contract](idx0d-contributions.md) and
[optional observation policy](../semantic-observe.md). The accepted V8
resource and typed-refusal contracts govern computation, retention and capture.
WSP0 supplies the end-to-end resource contract for client-visible operations.
None of these authorities is replaced here.

## Intent and ownership

Required outcomes are equivalent fresh and incremental answers, bounded
candidate retention, exact basis validation, safe cancellation and publication,
deterministic budget outcomes, and zero attributable work for disabled or
undemanded capabilities. A cache hit, successful process exit, unchanged version
string or quiet queue is insufficient evidence for any of those outcomes.

Forbidden outcomes are stale delivery or admission, degraded values promoted
to Complete, incomplete enumeration treated as absence, backend-free
TypeScript fact reuse, per-vertical flights, a second TypeInfo store, and a
performance claim obtained by doing less required work.

| Authority                                      | Sole responsibility                                                                                                                      | Receiving owner                                                                           |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `verter_identity`                              | Canonical nominal identities, collision-checked encoding, `QueryIdentity<Q>`, `SemanticFlightKey<Q>`, `InputBasisId`, `ResultContractId` | VID0T refines the orthogonal profile identities; no successor substitutes a key           |
| Rev11/TCM1 and TCM2/TCM3                       | Prepared products, exact map correspondence, projection and certified engine boundary                                                    | CPF1 consumes them; ENCT0 verifies coordinates and reopens the existing owner for residue |
| G2 shared cache runtime and `ProjectTypeStore` | Candidate slots, read-side validation, flight admission and publication fences                                                           | Successor operations consume it; PER0 verifies it                                         |
| SKR-READS and owning semantic producers        | Required read envelopes, `BudgetProfile`, operation scopes, receipts and typed refusal reuse                                             | Product operations adopt the existing envelope; no adapter invents a budget engine        |
| SKR-P-FLIGHT                                   | Adopt or reject measured compatible resolution sharing; retain one surviving resolution sharing authority                                | PM2 consumes the surviving authority, without a second coalescer                          |
| SKR-LIMITS                                     | Numeric operation allowances, retention and new-root admission calibration, policy revisions                                             | Consumers reference the adopted profile; this file restates no values                     |
| SKR-OBS                                        | Capture selection and elimination of optional bookkeeping                                                                                | Consumers preserve required state with capture off                                        |
| PER0                                           | Runtime invalidation, cancellation, equivalent-work and bounded-retention verification; evidence-run manifests                           | `PER0-AC2`–`AC4`, `PER0-AC-RUN`                                                           |
| UAO0                                           | Executable ownership validation and final observation/performance reconciliation                                                         | `UAO0-AC-R1`; no machinery is added here                                                  |

The G2 flight identity contract remains binding whether SKR-P-FLIGHT adopts or
rejects a particular sharing optimization. There is no requirement to retain
a rejected prototype. Mechanism changes do not create an alternative identity
authority. Missing runtime behavior returns to its implementation owner; PER0
and the locks verify and delete nothing.

## Identity consumption

**L01 — pure artifacts.** Consume the producer's existing typed artifact key.
PAR0 `PD03` makes parse interchangeability exactly
`(ContentId, CarrierProfileId, ParserId, ParserGrammarEpoch, SyntaxProfileId)`;
the syntax profile includes normalized options and dialect. Content alone,
content plus path, or a framework family is insufficient. Carrier-only parsing
does not acquire a semantic release or backend identity. Prepared declaration,
compiler and map products retain their own complete producer keys, options and
contracts. A semantic consumer records those products as value-side evidence;
it does not copy their version dimensions into a semantic lookup key.

**L02 — source, profile and map.** Source lineage is `SourceUnitId`, with
`SourceRevision` and `ContentId` beside it; mapped positions additionally retain
the exact `MapRevision` and map chain. All core positions are checked UTF-8
byte ranges in their declared domain. Requester encoding is adapter-only.
`CarrierProfileId`, exact `FrameworkProfileId`/`ReleaseId`, project profile,
PM-owned configured project, catalog and schema identities keep VID0's
orthogonality. Backend identity cannot enter a source or syntax identity.
Missing captured configuration, activation, membership or source evidence is
`NeedInputs`, never a default or complete-empty answer.

**L03 — canonical question.** Import `QueryIdentity<Q>` unchanged. TIF0
`TR15` supplies the descriptor tag, the digest of the canonical selector subject
and semantic arguments, the canonical observed-profile set and
`ResultContractId` to `QueryIdentity::compose`. Distinct answer-changing modes,
paths, substitutions, closure demands and exact semantic profiles are distinct
questions. Presentation, serialization, requester encoding and execution limits
are not. Backend artifact, process, serving lease, source revision, snapshot and
map generation belong to provenance or fences, not this lookup identity.
TypeScript interpretation contributes `TypeScriptSemanticProfileId`, not a
reported version string as a proxy for the serving backend.

The current certified binding's `observed_profile()` encodes serving facts as
well as interpretation. That existing binding remains the certification owner;
TIF1 must consume TIF0 `TR05`'s separation when adopting the façade. This is
not permission to copy serving fields into a successor query key or to weaken
`publish_admitted`/`serving_admitted`.

## Three separate contracts

**L04 — candidate lookup.** `QueryIdentity<Q>` is a snapshot-independent
lookup into bounded candidate slots. Lookup discovers possible values, never
certifies them. A source edit or backend restart may leave the question equal
while invalidating every candidate it finds. Candidate selection validates each
candidate's full evidence against the request's current view; it never chooses
the newest or first candidate without validation. Suspected digest collisions
compare full canonical bytes. Version hashes and dependency signatures must
not enlarge this key to avoid validation.

**L05 — flight.** A single-basis fact uses the existing G2-owned
`SemanticFlightKey<Q> = (QueryIdentity<Q>, InputBasisId)`. The basis is the
native captured view's `InputBasisId` or, for a TypeScript-authoritative child,
the certified `PublishSnapshot::input_basis`. Equal candidate keys with unequal
bases do not join by default. A surviving compatible-sharing mechanism may
share only under its owner-proven context compatibility and independent
adoption checks; a module spelling such as `./types` proves neither owner nor
world compatibility. There is no per-profile, per-vertical or per-adapter
singleflight beside the shared runtime.

A composed result has no single `InputBasisId` and no `SemanticFlightKey`.
It retains ordered child results, each with its own query identity and basis;
the sequence is not a new basis type, alias, wrapper or aggregate flight key.
Concurrent compositions share only their child flights. A composed cache entry
is scoped by its descriptor, child identities and each child's basis, isolated
by profile, edit and evaluation context (TIF0 `TR32`). It cannot satisfy a child
query, and a child cannot satisfy the composition. It lives in `ProjectTypeStore`.

**L06 — candidate/result provenance.** Every reusable value retains enough
owner-produced evidence to revalidate the complete observation basis:

- Query kind, semantic subject/arguments, observed profiles, result contract
  and producing authority; decl/role slots and the single-basis fact's
  `InputBasisId`. Composition retains this evidence on each child.
- Exact self roots and `ReadSetSignature` facts, including absence and
  membership/population witnesses, consulted environments/options, prepared
  products, source lineage/revision/content and position/map basis as relevant.
- Native owner/view and immutable overlay context; selection, demand,
  catalog/schema and profile generations actually observed. The full
  validation token proves currency; a narrower compatibility token only
  separates concurrency lanes and cannot certify a value.
- For TypeScript facts: the certified backend interpretation, actual backend
  artifact identity/integrity evidence from its owner, project/program binding,
  process/provider incarnation and serving lease, required version, published
  source/content/map basis and query position. The same reported TypeScript
  version with different backend artifacts is not reuse evidence.
- Per-fact quality and machine causes, `ReuseClass`, required logical cost
  receipts/operation footprints, recovery and requested diagnostic data.

These are relevance-qualified requirements, not fields to manufacture on every
artifact. A native no-projection artifact has no TypeScript backend dependency;
an actual TypeScript-dependent result cannot claim that exemption. Required
evidence remains available with optional capture off. Public DTOs contain owned
provenance, never provider handles, live pointers or serving capabilities.

The existing certified seam compares the published snapshot basis and serving
identity/lease before the store write. `PublishSnapshot::input_basis` covers
project, resolution-map version, filesystem generation and published file rows,
including content/map bytes and structure stamps. A duplicate provider URI is
refused. Its basis does not by itself certify a backend artifact or live lease;
all serving evidence must also validate. An owner lacking complete evidence
cannot admit a value. Repair the owning provenance boundary; never enlarge
`QueryIdentity` to reconstruct it.

## Publication, cancellation and lifetime

**L07 — admission and delivery.** Capture the owner view and plan at admission;
record exact reads during computation. Immediately before publication, validate
strict roots, all read evidence, current source/map/project/profile and demand
generations, certified serving state where applicable, and cancellation. Publish
the immutable value and its evidence atomically through the existing completion
fence. Revalidate at delivery; a follower validates the winner against its own
view, profile and cost context. A completed producer is not an admitted result.
Missing, torn or non-current evidence returns a typed unsettled/stale outcome.
It never becomes semantic absence or a cache hit.

**L08 — cancellation and supersession.** Observe cancellation at demand-plan
boundaries and the engine's `CancellationCheckpoint`, including before publish
and delivery. A cancelled plan publishes no value, refusal, reverse-index
metadata or persistent artifact. A generation superseded before admission is
not admitted; already admitted work is cancelled or completes ReturnOnly for
internal settlement and is never served after the newer epoch is observed.
Completion order cannot overwrite a newer generation. Bounded retries report
`Unavailable { UnsettledInput }` or the owning typed stale/aborted cause on
exhaustion. Sleeps, idle periods and empty queues cannot prove completion.

Cancellation belongs to the cancelled demand. A follower releases its
subscription/reservation without cancelling independent live subscribers. If
the producer obligation aborts, panics or is superseded, wake affected followers
with the runtime's typed abort/retry disposition; never broadcast a retained
ReturnOnly value as a reusable winner. Each surviving follower either adopts a
validated published result or computes under a fresh safe claim. No wait may
retain an ownership dependency that makes the producer wait on that follower.
Reservations, leases and producer ownership settle exactly once on every path.

**L09 — overflow and pressure.** Checked budget arithmetic, identity/generation
counters and bounded population sizes cannot wrap, saturate into a valid
identity or silently truncate to Complete. The responsible owner refuses
admission with a typed cause and retires/supersedes the affected generation or
returns an explicitly partial enumeration, as its contract requires. No
reusable value can be minted from overflowed evidence. Logical budget exhaustion
and retention pressure are different: a Complete result denied retention is
returned Complete and uncached. Pressure does not truncate admitted computation,
revoke a caller's result or create a deterministic cached budget refusal.

**L10 — bounded retention.** Candidates, profile variants, refusal entries,
receipt/evidence pages, interned identities, staged contributions and flight
tables obey the existing family caps and aggregate ownership account.
Count caps and byte accounting complement one another; neither supplies an
unbounded exception to the other. Live reader/external pins retain their charges
after eviction, shared payloads are charged once and public owned copies in
full. Ownership transfers and cancellation/release paths release charges once.
New roots may be backpressured under the owning policy; admitted dependencies
need no second root permit. There is no hard computation-memory or RSS cap
promised by this law. Optional soft construction rails cannot reject already
Complete output. Numeric policy is SKR-LIMITS-owned.

## Quality, refusal and incremental equivalence

**L11 — no degraded warming.** Cancellation, supersession, torn/unsettled input,
stale backend, partial index enumeration and budget-truncated values are
ReturnOnly: no fact/result/plan entry, reverse metadata or persistent value.
Ambiguous or missing-input outcomes cannot certify absence. A Complete child
does not make its degraded parent eligible; independently complete children
need their own exact demands and evidence. IDX0's staged delta publishes only
a complete, fenced partition and atomic forward/reverse postings.

Semantic quality and reuse eligibility remain independent. An owner-proven
deterministically Approximate fact may be Shared (TIF0 `TR25`); an interruption-
caused partial value cannot. Certified budget-independent divergence keeps its
existing proof rules. This does not admit arbitrary unsupported or unresolved
results.

**L12 — typed refusal reuse.** The V8/SKR-READS exception is a separate refusal
class, never a degraded fact cache. A deterministic fresh-operation refusal
retains exact input, checker entry/reset/depth state, `BudgetProfile`, failed
prefix, prospective charge, dependency/absence evidence and recovery. A spent
nested allowance is not context-free evidence: lift it to the owning operation
root or use the full relevant state under the adopted runtime contract.

A connected-ledger refusal is eligible only at an isolated root with a fresh
empty ledger and canonical normalized demand. DEM0 `DR19`'s identity includes
demand purpose, BudgetProfile, ordered framework profiles, normalized root
context, demand epoch and root source identity (revision/content, or map basis).
Relevant environments/options, checker assumptions/state, batch membership and
logical order also remain exact. Ambient/nonempty ledgers and nondeterministic
contexts are ReturnOnly. Such a root refusal never answers a subquery.

Same-scope/profile repetition may validate the sealed refusal and apply bounded
receipts without rerunning semantic producers. It remains a refusal. A changed
profile, edit, root/context or epoch misses; a larger allowance may recompute
and heal. Cancellation, supersession, unfinished flights, allocation/admission
pressure and optional soft-byte refusals are Never, not refusal candidates.
Refusal/profile candidates obey ordinary lifetime and retention bounds.

**L13 — incremental equals fresh.** On one current captured basis, identical
descriptor, semantic demand, normalized context and budget profile must produce
the same facts, per-fact quality, ambiguity/refusal class, diagnostic/recovery
contract and authored origins under fresh, cold, warm, mixed and follower
execution. Logical charges and refusal points agree after canonical receipt
adoption, independent of cache warmth, worker count and scheduling. Presentation
may differ only under its separate requested profile. A relevant edit invalidates
the affected evidence; unrelated edits preserve validated work. Backend-only
changes invalidate TypeScript-dependent observations and preserve independent
native/syntax artifacts. A retry after cancellation agrees with a fresh request,
without reusing a cancelled prefix.

## Per-operation budget classes

**L14 — placement.** DEM0's purpose (`Interactive`, `DiagnosticsPublication`,
`BatchCheck`, `IndexContribution`, `Compile`, `Explain`) selects the adopted
SKR-READS BudgetProfile at root admission. Purpose never selects a different
semantic answer. The profile covers relevant operation/connected allowances,
checker reset/depth policy, cost-unit/model and policy revisions. Changing a
numeric allowance or those policies changes immutable profile identity and
refusal eligibility, not `QueryIdentity`, `ResultContractId` or `SemanticFlightKey`.
New operations must declare their class and exact fact demands before execution.
The classes below are descriptor obligations, not new enums or limit values.

| Class                  | TIF0 descriptors                                                                                                                        | Charged work and scope                                                                                                                                                                                         |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Symbol inventory       | OP08 `ListSymbols`                                                                                                                      | Bounded shallow declaration/occurrence enumeration; no semantic expansion; incomplete enumeration cannot be complete-empty                                                                                     |
| Native observation     | OP01 `ResolveSymbol`, OP02 `ProjectPath`, OP03 `EvaluateTypeExpression`, OP04 `ExpandAround`, OP09 `ShallowSurface`, native OP10 facets | Fresh owning operation scope for resolution, instantiation, reduction, requested projection/closure and output work; nested memo calls do not reset allowances                                                 |
| Flow/context           | OP05 `FlowNarrowingAt`, OP06 `ContextualTypeAt`                                                                                         | Exact source/position and checker flow/context state, demanded branches and required recovery; no file-wide eager analysis                                                                                     |
| Relation               | OP07 `Relate`                                                                                                                           | Checker-exact entry/reset/depth and relation footprint; preserve Maybe/diagnostic behavior, never substitute a guessed hard depth                                                                              |
| TypeScript observation | OP11 `TypeAtPosition`, OP12 `DeclarationAtPosition`, TypeScript OP10 child                                                              | Certified engine operation, projection/map validation, requested result materialization; external provider work remains in the end-to-end resource envelope                                                    |
| Composition            | Composed OP10, OP11, OP12                                                                                                               | One normalized connected demand; each child retains its own operation scope/receipt; bounded composition/serialization charged once; no composed flight or second budget engine                                |
| Non-TypeInfo products  | Compile, index, rules/actions, editor/public/agent operations                                                                           | Their catalog descriptor and surviving producer define parse/resolve/plan/emit/map/visit/output demands; read the same budget/receipt and ownership law, without routing unrelated operations through TypeInfo |

**L15 — required logical cost.** Seal exclusive costs and actually consumed
immediate prerequisites in shared receipt DAGs/SCC seals. Charge each logical
identity once per connected demand in canonical adoption order, retaining
operation footprints and checker transforms. Warm adoption replays logical cost;
it does not get a free allowance. Interrupted replay cannot mark unpaid children
paid, poison a Complete candidate or borrow another caller's remaining allowance.
Followers independently validate their profile, entry state and charges.
Required receipts are production state with real cost; chronological traces are
not receipts. Budget failure stays typed, preserves diagnostic/recovery rules,
and cannot invent a checker diagnostic.

## Audit contract and zero work

**L16 — capture.** The following are semantic audit event obligations, not new
runtime event types. Producers use the existing audit envelope when capture is
available and requested. Required compact status, validity, budget receipts,
ownership, cancellation and admission state survive capture off. OPTIONAL
payload construction, correlation IDs, timers, contention/RSS/attribution
histories and footprint mining follow the default-off observation policy.
Unavailable capture is reported unavailable, never fabricated zeros.

| Event boundary             | Captured evidence                                                                                                                                                  | Owner                                           |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------- |
| Activation/demand planning | DEM0's one `ActivationPlanned` per source plan and one `DemandPlanned` per request/batch, epochs, purpose, selected/dormant regions, exact capability/fact demands | COX0                                            |
| Candidate validation       | Descriptor/query identity, candidate basis, hit/miss and typed invalidation cause; no serving handles                                                              | Shared cache runtime; TIF1 consumes             |
| Flight settlement          | Producer/join/retry/abort disposition, exact basis and owner-compatible context; every subscriber has a terminal outcome                                           | G2 runtime; SKR-P-FLIGHT for resolution sharing |
| Operation settlement       | Scope/profile, quality/reuse class, sealed logical usage and failed-prefix/refusal cause or recovery                                                               | SKR-READS and operation producer                |
| Publication/delivery       | Admitted, ReturnOnly, cancelled, superseded, stale-backend, overflow or retention-denied disposition with relevant fences                                          | Existing publication owner                      |
| Retirement                 | Charge/lease transfer and terminal release, current occupancy when requested; no duplicate historical counter stream                                               | Existing lifetime/retention owner               |

**L17 — attributable work.** Disabled, dormant, ambiguous, unproven or withdrawn
regions do zero profile projection, script-fact, TypeInfo and contribution work.
Selected-but-unrequested capabilities do zero capability work. Shared carrier
parsing required by another demand is attributed to that demand, never billed
as disabled-profile work. Capture off adds zero observation-only work while
retaining correctness state. Do not count missing instrumentation as zero work.
Discriminating controls use available opt-in work counters and required current
occupancy, and verify both affected and unaffected profiles.

## Consumer and displaced-site inventory

Rows name concrete existing sites or explicitly future contract sites. Related
objects in a row share a responsibility; the row is not a license to merge their
keys or bounds. Existing imported substrate is retained, not designated a
duplicate merely because several families use it. Receiving owners reconcile
this inventory with their final implementation, including L4 changes. PER0
checks every row's provenance, sharing, boundedness and deletion disposition;
UAO0 validates the table under `UAO0-AC-R1`.

| Site / consumer                                                                  | Current owner or evidence home                                                                                         | Sole successor contract / disposition                                                                                                                                                                                                 | Implementation or deletion owner; later verification                                                                     |
| -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Canonical candidate/flight keys                                                  | `verter_identity/src/identity.rs`                                                                                      | Retain `QueryIdentity`/`SemanticFlightKey`; three-contract split L04–L06                                                                                                                                                              | Existing identity/G2 owner; TIF1 `AC2` consumes; PER0 `AC2` verifies                                                     |
| File parse/shallow/prepared artifacts and compile slots                          | `verter_session/src/file_artifact_store.rs`, `cache.rs`, `carrier_publication_store/`; language parse identities       | Consume producer keys, PAR0 equality and exact carrier profile; no semantic/backend pollution of pure syntax                                                                                                                          | CPF1 `CPF1-AC1`, referenced VID0 `V-C01`/UAK0 `D08`; PER0 `AC2`                                                          |
| Retained OXC snapshot shards                                                     | `verter_semantic_source/src/decl_lowering.rs`, `parsed_eval_program.rs`; `verter_session_query/src/source/snapshot.rs` | Retain one parser-free snapshot cache/lease owner; replace incomplete parse reuse identity, not the cache with a second parser                                                                                                        | FWC1 `FWC1-AC5`, PAR0 `PAR-D03`; UAI0 and PER0 `AC2`                                                                     |
| Semantic family, named-type, relation, derivation and result evidence stores     | `verter_type_engine/src/semantic_query_memo/`, `project_semantic_dispatch/`, `bounded_query_retention.rs`              | Retain shared family/runtime, read validation, receipt DAGs and bounds; successor entry paths cannot add parallel materializations                                                                                                    | SKR-READS, SKR-FACTS-2, SKR-EVID-1/2 and SKR-OWN-3 retain their mechanisms; PER0 `AC3`/`AC4`                             |
| Cache-runtime and semantic flights                                               | `verter_type_engine/src/cache_runtime/singleflight.rs`, `singleflight_publish.rs`, `semantic_query_memo/inflight.rs`   | Existing shared runtime, owner/view-separated flights and follower validation; no successor-local flight                                                                                                                              | G2/SKR runtime owner; PER0 `AC2`/`AC3`                                                                                   |
| Component-meta final result and public-API projection                            | `component_meta_result_db`, session `resolver_core/component_meta_query_engine/`, `get_public_api`                     | TypeInfo/ComponentInfo view on ProjectTypeStore; remove duplicate semantic authority, preserving shared backing mechanisms                                                                                                            | TIF1 `TIF1-AC1`/`AC2`/`AC3`, UAK0 `D12`/`D13`; PER0 and UAO0                                                             |
| Off-store framework-surface caches/coalescing                                    | session `typeinfo/framework_surface/`, `FrameworkSurfaceStore`                                                         | Delete off-store surface cache/authority; demanded facets execute through TypeInfo and shared child flights                                                                                                                           | TIF1 `TIF1-AC1`, UAK0 `D14`, DEM0 `DEM-D03`; PER0 verifies deletion                                                      |
| Scratch expression URI/LRU                                                       | session `typeinfo/scratch_cache.rs`, expression synthesis                                                              | TIF0 OP03 uses the canonical operation and read envelope; retained syntax resources cannot become a second answer store or retain degraded nodes                                                                                      | TIF1 `TIF1-AC2`/`AC3`; PER0 `AC3`                                                                                        |
| Certified serving warm state                                                     | session `external_ts/warm_cache.rs`, `semantic_capability.rs`, `external_ts/engine.rs`                                 | Retain binding/mode/lease owner; serving state is not a semantic fact cache; adopt exact interpretation/provenance split                                                                                                              | TIF1 `TIF1-AC2`/`AC3` consumes TIF0 `TR05`/`TR29`; PER0 `AC2`                                                            |
| Provider establishment and foreground synchronization                            | LSP provider hubs and `sync_orchestration.rs`                                                                          | Retain provider-process lifecycle and document synchronization ownership; establishment/repair coordination cannot cache semantic answers or become a vertical observation flight                                                     | Existing LSP/provider owner; COX0 consumes demand, PER0 `AC2`/`AC3` verifies settlement                                  |
| Compact maps, content mapping and pinned provider surfaces                       | compiler `code_transform/`, session `content_mapper.rs`, LSP `provider_surface_store/`                                 | Retain TCM mapping authority and current-basis surface pins; no adapter mapper cache with guessed correspondence                                                                                                                      | TCM1/TCM2/TCM3 and existing surface owner; ENCT0 verifies coordinates, PER0 `AC2`/`AC4` verifies reuse                   |
| Workspace resolution caches/flights and currency                                 | workspace `fact_cache.rs`, `filesystem.rs`, `resolution_flights.rs`, `resolution_currency.rs`; resolution attempts     | Keep the measured surviving SKR sharing authority and owner-qualified worlds; delete rejected prototypes; later PM semantics reuse it                                                                                                 | SKR-P-FLIGHT adoption/deletion, PM2 integration; PER0 `AC2`/`AC4`                                                        |
| Contribution postings, query pages and private component scans                   | IDX0D contract; current per-request component scans                                                                    | One atomic index on complete producer contributions; partial pages ReturnOnly; delete consumer-private full scans                                                                                                                     | IDX0 `IDX0-AC1`–`AC4`, UAK0 `D18`; PER0 `AC3`/`AC4`, UAO0                                                                |
| Activation, claim and demand plans                                               | DEM0 contract; current framework registration/selection and eager LSP diagnostics                                      | Epoch-qualified demand, zero undemanded work; retire eager selectors/duplicate plan authorities, not independent diagnostics                                                                                                          | COX0 `COX0-AC1`–`AC4`, DEM0 displaced routes; PER0 `AC3`/`AC4`                                                           |
| Captured configuration, package activation, roles and project overlays           | CFG0/FWA/EAK/PPR contracts                                                                                             | Owner-produced snapshots/read evidence; no local live-config, role or package-result cache as semantic authority                                                                                                                      | CFG0's receiving CFG1 assignment, FWA1, EAK1, PPR0T and PM1 keep the referenced contracts; PER0 `AC2`                    |
| Logical operation/connected budgets and root refusals                            | type engine `request_budget.rs`, `project_semantic_dispatch/cost_receipt.rs`, `dispatch_txn.rs`                        | Adopt SKR envelopes, immutable profiles and canonical receipt replay; retire raw-value/charge bypasses in their owning migrations                                                                                                     | SKR-READS `AC1`–`AC4`, SKR-FACTS-2; SKR-LIMITS calibration; PER0 `AC3`/`AC4`                                             |
| Retention, evidence/profile interning, external pins and new-root admission      | `verter_session_query/src/retention/`, shared bounded retention and existing ownership stores                          | One aggregate ownership account plus family caps; bounded refusals and profile variants; Complete uncached on pressure                                                                                                                | SKR-OWN-3, SKR-EVID-1/2 and SKR-LIMITS; PER0 `AC4`; WSP6 current counts                                                  |
| Audit finalization, attribution and footprint capture                            | session `host_audit_runtime.rs`, `request_observers.rs`; `verter_audit`                                                | Required state separated from default-off capture; no successor always-on trace/timer                                                                                                                                                 | SKR-OBS classification/elimination; PER0 `AC4`, UAO0                                                                     |
| Public Rust/FFI/NAPI/WASM/LSP/MCP/CLI result projections                         | PUB0 contract and existing adapters                                                                                    | Owned typed outcome/provenance and bounded projection only; no public cache, coalescer, semantic engine or refusal promotion                                                                                                          | PUB0 specifies, PUB0T implements/cuts over; UAP0 verifies public contract, PER0 validates reuse                          |
| Manifest descriptor generation                                                   | VIM1, catalog descriptors                                                                                              | Generate exact identity inputs, demands and budget class references; add no runtime budget registry or coalescer                                                                                                                      | VIM1 `VIM1-AC1`/`AC2`; UAM0 governance, UAO0 ownership                                                                   |
| Compiler work ledger and oracle genesis                                          | CPER0 and compiler prepared/direct/managed routes                                                                      | Equivalent outputs/diagnostics/maps under exact compilation contracts; modes recorded, existing prepared artifacts shared                                                                                                             | CPER0 locks methodology, CMP1 and compiler owners implement; CPER0B evidence, PER0 consumption                           |
| Framework conformance oracle/domain caches                                       | `packages/framework-conformance-harness`, FCH1                                                                         | Hermetic pinned domain/artifact closure and exact oracle observation; test caches are not production semantic authorities                                                                                                             | FCH1 `FCH1-AC1`–`AC4`; PER0 equivalent-answer cells                                                                      |
| Every other registered capability, framework/document/DSL vertical and extension | Catalog descriptor, producer-local contribution schema and owning implementation charter                               | Uses the relevant rows above; before adding a local retaining site the owner inventories its exact identity, provenance, lifetime/bounds and shared mechanism. A duplicate local cache/coalescer is removed in that same owner change | The descriptor's producing implementation node owns deletion; PER0 `AC1`/`AC4` and UAO0 `AC-R1` verify complete coverage |

The last row is a universal consumer obligation, not an unnamed deletion
deferral. New sites must become concrete receiving inventory rows when their
producer exists. The current head has no composed TypeInfo flight/basis/store,
universal contribution API or successor per-vertical coalescer to delete.
Their contracts prohibit introducing them. No existing cache is deleted by this
decision. The UAK0, VID0, PAR0, DEM0 and TIF0 displaced routes retain their
already assigned deletion owners; this inventory references rather than re-owns
them. An implementation owner removes its displaced route in its atomic
cutover. PER0 reopens residue to that owner instead of promising later deletion.

## Case table for PER0

These are receiving specifications under `PER0-AC2`–`AC4`, not executed tests or
claims that every successor mechanism is already present. Prefer extending the
existing discriminating guards listed below. Each case includes an affected
input and an unaffected/control demand where needed; successful lookup or
process exit alone never proves equivalence.

| Case | Input / control                                                                                                   | Required outcome and rejected outcome                                                                                                                                                          |
| ---- | ----------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C01  | Same semantic request, new captured snapshot; unrelated edit control                                              | Equal QueryIdentity; candidate discovery is possible, but basis/read validation decides reuse. No snapshot field added to lookup key                                                           |
| C02  | Equal question, distinct InputBasisId; concurrent same-basis control                                              | Distinct flights across incompatible bases; one safe producer for compatible identical cold demand; followers validate independently                                                           |
| C03  | Same reported TS version, different actual backend artifact; unchanged native parse control                       | Old TypeScript fact cannot pass provenance validation or warm; native no-projection artifact stays reusable                                                                                    |
| C04  | Provider/process restart or serving-lease rotation with identical bytes; live-session control                     | Prior serving certification refuses adoption/publication; new certified request equals fresh; version equality does not preserve it                                                            |
| C05  | Change configured project/program, resolution/config/library fact or overlay; disjoint owner control              | Invalidate exactly observed evidence; no cross-project/overlay leakage or first-project answer; unrelated native evidence remains usable                                                       |
| C06  | Edit source, including edit away and back; unrelated-source control                                               | Revision/fence and strict-root validation reject old in-flight work; current result equals fresh even when content returns to prior bytes                                                      |
| C07  | Change map revision/chain only; unmapped native control                                                           | Reject position-derived old fact/selector; no clamping/remapping; map-independent artifact remains reusable                                                                                    |
| C08  | Change parser ID, grammar epoch, dialect or normalized option; another-carrier control                            | Reject affected parse and dependent results under PAR0 equality; preserve other carrier products                                                                                               |
| C09  | Change release/profile/catalog/schema/activation or participation epoch rapidly; stable-demand control            | Exact question/profile changes where semantic; stale demand admission/delivery refused; only current plan publishes                                                                            |
| C10  | Cancel before admission, while computing, or immediately before publish/delivery; fresh retry                     | No cancelled value/refusal/reverse metadata; reservations settle once; retry agrees with fresh                                                                                                 |
| C11  | Cancel follower while another remains; producer panic/abort and stale winner controls                             | Independent follower remains safe; all affected waiters wake/terminate or bounded-retry; no ReturnOnly winner broadcast or ownership deadlock                                                  |
| C12  | Checked generation/cost/population overflow; ordinary boundary control                                            | No wrapped identity, stale admission or truncated Complete; typed refusal/partial status from owning contract                                                                                  |
| C13  | Disable, withdraw, leave ambiguous/unproven, or select without requesting profile; active profile in same carrier | Zero attributable projection/script-fact/type/index or capability work; independent requested behavior preserved; shared parse attributed to its real demand                                   |
| C14  | Equivalent fresh/cold/warm/mixed/follower requests and deterministic schedule/worker permutations                 | Same semantic facts, quality, causes, recovery/diagnostics, origins and logical charge/refusal point; no warm-only healing from unpaid work                                                    |
| C15  | Same eligible isolated-root refusal/profile repeat; subquery/nonempty-ledger control                              | Reuse refusal with zero semantic producer execution; validation/receipt work allowed; no exact absence, Complete speedup or subquery reuse                                                     |
| C16  | Larger/different budget profile, checker entry/reset/depth, batch order or root epoch/edit                        | Old refusal misses; recompute/heal where capable; complete subresults remain independently reusable                                                                                            |
| C17  | Diamond/SCC/shared-child receipt adoption, mixed warmth and interrupted replay                                    | Charge logical identities once in canonical order; unpaid descendants remain unpaid; cancellation cannot seal a reusable failed prefix                                                         |
| C18  | Partial contribution or bounded candidate walk; changed membership/population and stale prior postings            | No prefix/negative/membership cache entry; complete delta publishes atomically; a cached child refusal does not warm the partial parent                                                        |
| C19  | Native/TS composed observation, child basis edit and concurrent requests                                          | Preserve child bases/authorities/order; only child flights share; child/composed entries never satisfy one another; stale child prevents full admission                                        |
| C20  | Repeated bounded live set with source/profile/refusal churn, eviction, close and reader/pin release               | Candidates, profiles, flights, evidence and backing occupancy return to live-set bounds; no permanent negative interner, orphan lease or retained version history                              |
| C21  | Retention/new-root pressure, then release; already admitted computation and live-reader controls                  | Complete output returned uncached; no memory-cap truncation or ownership revocation; backpressure affects only permitted new admission                                                         |
| C22  | Capture on/off/unavailable with identical required inputs                                                         | Identical answers, budgets, validity and lifetime; off performs zero optional work; unavailable metrics/capture remain unavailable                                                             |
| C23  | Vary unrelated workspace size, demanded population and sibling fan-out, holding scope/budget fixed                | Counted work follows demanded/visited population, not full workspace scans or duplicated parse/resolve/emit; incomplete enumeration remains typed                                              |
| C24  | Client-visible request, diagnostics publication and compiler outputs under equivalent corpus/options              | Account for generation, provider, queue, serialization, client application/paint and process-tree memory where observable; preserve all required features and diagnostic/map/runtime contracts |

## Measurement and evidence-run cells

**L18 — equivalent work first.** Acceptance uses semantic answer classes,
deterministic work counts and growth ratios. A refused/partial/uncovered answer
is recorded in that class, never ranked as faster complete work. Growth compares
the demanded corpus across sizes; a fixed-budget truncated walk cannot stand in
for a complete larger-population comparison. Unrelated-population controls
detect hidden eager work. Cold, warm, mixed, batch, thread and cache/map modes
are explicit, including validation/projection versus producer execution.

When comparison is needed, baseline and candidate run on the same assigned
worker in the same session, interleaved, preserving equivalent answers and the
frozen comparison workload. There is no acceptance requirement for historical
Git objects or stored receipts. Runtime candidate/base identities, toolchain,
platform, dependency integrity, corpus and configuration belong in run evidence,
not refreshed proof pins or timing/count transcripts in this document.

Timing, allocation and memory are evidence, never a local wall-clock gate.
Unavailable metrics are recorded with their reason and do not fail performance
acceptance. This governing measurement rule also covers older machine-specific
requirements. Absolute production time/memory gates belong only to their
performance-phase owners. PER0 registers and executes the named
`per0-equivalent-work` and `per0-rss-soak` evidence runs on `bench-m3`; this
decision neither writes those manifests nor starts a benchmark/soak.

PER0 owns `scripts/benchmark/evidence-runs/per0-equivalent-work.json` and
`per0-rss-soak.json`, freezes them before execution, and validates their
SKR-PERF-H format with the existing evidence-run dry-run. The following cell
groups are predeclared inputs to those manifests, not numeric limits:

| Group                    | Run / cases                              | Required cells and controls                                                                                                                                                                                                     |
| ------------------------ | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1 identity/invalidation | `per0-equivalent-work`, C01–C09, C19     | Native, TS and composed answer classes; backend artifact/process/project/source/map/parser/profile changes; independent native/carrier control; candidate lookup and producer/join counts                                       |
| E2 settlement/refusal    | `per0-equivalent-work`, C10–C12, C14–C18 | Cancel/abort/supersede/overflow outcomes; same-profile refusal repeat and larger-profile heal; exact diagnostics/recovery; fresh/cold/warm/mixed/follower and diamond/SCC logical/producer work                                 |
| E3 demand/scaling        | `per0-equivalent-work`, C13, C22–C24     | Disabled and undemanded work, unaffected capability answers; unrelated-size and demanded-size growth, sibling fan-out, batch/thread/cache/map modes; required versus optional work                                              |
| R1 bounded churn         | `per0-rss-soak`, C20                     | Fixed bounded live sources with edit/profile/refusal/candidate churn; live table/backing counts, evidence/profile/flight residency, leases and charges before/after readers close; RSS/process-tree and allocation observations |
| R2 pressure/lifetime     | `per0-rss-soak`, C10–C11, C21–C22        | Complete uncached delivery, new-root backpressure, cancellation and retry, retained/external pins and ownership transfers; capture on/off controls; explicit unavailable cells                                                  |
| R3 end-to-end            | Both runs, C24                           | Equivalent requested generation/diagnostics/output, provider work/process costs, queued/outbound bytes, serialization and actual client apply/paint when claiming real-client behavior; otherwise client metrics unavailable    |

Each manifest binds concrete scenarios/corpora, baseline/candidate arms, modes,
required cells, repetition and noise methodology before measurement. Reports
retain failures and uncertainty rather than weakening the workload after seeing
results. Compiler-only microbenchmarks make no client responsiveness claim.
PER0 verifies the surviving flight owner and adopted SKR-LIMITS profiles; a
contract conflict returns here for an amendment, never a silent local mechanism.

## Acceptance evidence and handoff

- **PER0D-AC1:** the inventory names sole cache/flight/budget authority,
  retained substrate, displaced-site deletion owner and verifier. UAO0 owns its
  executable ownership validator (`UAO0-AC-R1`); PER0 verifies the final sites.
- **PER0D-AC2:** L01–L10 and C01–C13/C19 specify exact identity/provenance,
  cancellation and overflow. Existing evidence includes `verter_identity`'s
  `query_identity_excludes_basis_but_flight_key_includes_it` and
  `query_identity_is_a_profile_set_not_a_profile_list`, plus the
  canonical encoding tests; the cache-runtime tests
  `by_flight_key_keys_map_on_mapk_and_coalesces_on_flightk`,
  `cacheable_joiner_runs_validate_on_its_own_thread`,
  `cooperative_admission_post_compute_revalidation_drops_stale`, and
  `return_only_winner_not_broadcast_cross_view_joiner_forks`; and
  `external_ts/warm_cache_tests.rs`'s
  `each_key_dimension_change_is_a_miss` and
  `reconnect_generation_bump_makes_prior_entry_unreachable`.
  These cover existing boundaries, not all successor/backend-artifact cases.
- **PER0D-AC3:** L11–L15 and C14–C19 define incremental/fresh equivalence and
  degraded non-admission with the typed-refusal exception. Existing
  `non_cacheable_bundle_runs_one_cold_flight_and_replays_its_taint` and
  `cacheable_bundle_runs_one_cold_flight_and_then_warm_hits` distinguish
  request-only from shared reuse; receipt/refusal evidence lives in
  `project_semantic_dispatch` and `semantic_query_memo`. PER0 supplies the
  final cross-surface cases. Runtime TDD is not applicable here: no production
  or test behavior changes.
- **PER0D-AC4:** L16–L18 and E1–R3 bind zero work, deterministic work/growth,
  bounded retention and equivalent end-to-end measurement. No local timing,
  new counters or soak is justified by this prose change. PER0 owns the named
  evidence runs and final verification; CPER0, FCH1, PUB0 and VIM1 consume this
  law without waiting for those runs. WSP6 counts and optional capture policy
  keep their existing owners.

Final command outcomes, candidate identities and platform go in the worker's
review report, not this document. Markdown/link and scoped-diff inspection are
the applicable local document checks; the docs-site build excludes `arch/**`
and cannot validate this law. Source compilation, behavioral tests, public wire
generation, real-provider execution and benchmarks do not discriminate this
docs-only change. The workflow owns independent reviews and the pushed-head CI
gate. This decision adds no check and claims no runtime or product readiness.
