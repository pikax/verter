# TypeInfo query, selector and authority composition

This decision defines the canonical public TypeInfo façade: how a request
names what it asks about (selectors), what it asks (operation descriptors),
who answers (owner-routed plans), what the answer carries (authority,
provenance and per-fact quality), and how answers are cached. It consumes the
accepted observation and runtime identity law and redefines none of it:
`QueryIdentity`, `SemanticFlightKey`, `InputBasisId`, `ResultContractId` and
`CertifiedTypeEngineBinding` are imported unchanged.

Today, framework-shaped host/session registries and untagged public
boundaries own TypeInfo: five host entry vocabularies, path-string selectors,
an opaque parent-graph handle and a query echo that mixes semantics with
presentation. The final and sole owner is the typed immutable universal
catalog and the demand-selected kernel services. UAK0 boundary `B06` already
names this decision as the `TypeInfoRequest` contract and TIF1 as its
implementation (`TIF1-AC2`).

It describes the repository at `docs(arch): define two-stage activation and
the demand plan (#811)`, 2026-10-09. It follows the docs-only rule in
[README.md](README.md): it changes no production route and adds no check. It
reads the [identities](identities.md), [coordinates](coordinates.md),
[configuration](configuration.md) and [demand](demand-activation.md)
decisions, and does not re-own anything they, the
[authority inventory](authority-inventory.md) or the
[constitution](constitution.md) assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/TIF0/products/`:

| File | Holds |
| ---- | ----- |
| `typeinfo-inventory.v1.json` | Imported identities `TI01`–`TI12`, contract rules `TR01`–`TR32`, operation descriptors `OP01`–`OP12`, outcomes `TIF-O01`–`TIF-O10`, contract consumers `TIF-C01`–`TIF-C10`, displaced routes `TIF-D01`–`TIF-D06`, the UAK0, DEM0, VID0 and ENC0 routes it references, coverage of each deletion category, findings, empty populations, the existing coverage it cites and transferred obligations |
| `typeinfo-case-table.v1.json` | Cases `TC01`–`TC13`: input, required and forbidden outcome, the rules each case exercises, existing evidence, and the node whose test makes it executable |

Every `successorPath` starts at TIF0 and follows successor edges in the
controller-owned plan. UAO0 is reached through TIF1 and IDX0 through TIF1D.
UAK0, DEM0, VID0 and ENC0 rows keep their owners in those nodes' products.
`referencedRoutes` and `referencedConsumers` name the owning product and do
not copy an owner or a receiving acceptance.

## Imported identities

| Id | Identity | Home | Use here |
| -- | -------- | ---- | -------- |
| `TI01` | `QueryIdentity<Q>` | `verter_identity` | candidate-lookup key; TypeInfo supplies the `Q` marker and the semantic-argument digest |
| `TI02` | `SemanticFlightKey<Q>` | `verter_identity` | in-flight key for a fact whose basis is one `InputBasisId`; a composed result has none (`TR02`, `TR28`) |
| `TI03` | `InputBasisId` | `verter_identity`; minted by `InputBasis` and `PublishSnapshot::input_basis` | the basis of a native or TypeScript-authoritative fact |
| `TI04` | `ResultContractId` | `verter_identity` | per-descriptor result contract |
| `TI05` | `TypeObservationBasis` | none | a role name, not a type (`TR02`, `TIF-F01`) |
| `TI06` | `CertifiedTypeEngineBinding`, `BoundProject`, `EngineBackend` | `verter_session` | the only route to TypeScript-authoritative facts |
| `TI07` | `TypeScriptSemanticProfileId`, `PresentationProfileId`, `SerializationProfileId`, `ExecutionPolicy` | `verter_identity::profile` | keep presentation, wire shape and limits out of identity |
| `TI08` | `SourceUnitId`, `SourceRevision`, `ContentId`, `MapRevision` | `verter_identity` | selector subjects and source-revision bases |
| `TI09` | `FrameworkProfileId`, `ReleaseId`, `ProjectProfileId`, `ConfiguredProjectId`, catalog identity, `CapabilityId` | VID0 rows `I03`–`I10` | observed profiles, project selector, capability cells |
| `TI10` | `SourceByteOffset`, `SourceByteRange` (`CD1`) | `verter_span` | the only position unit a selector or provenance carries |
| `TI11` | `QueryOutcome`, `IncompleteReason`, `ResultCompleteness`, `BudgetProfile`, `OperationFootprint` | `verter_type_engine` | the SKR-READS delivery envelope a native fact rides |
| `TI12` | `ReuseClass`, `NoReuseCause`, `RefusalReplay` | `verter_session_query::facts::reuse` | reuse eligibility, independent of quality |

## Contract rules

### Imported observation identity (subblock 1)

- **TR01.** TypeInfo imports `QueryIdentity`, `SemanticFlightKey`,
  `InputBasisId` and `ResultContractId` from `verter_identity` and
  `CertifiedTypeEngineBinding` from `verter_session` unchanged. It defines no
  generic key, flight, basis or contract type, no alias of one and no wrapper
  that adds a field to one. Its only additions are the per-descriptor
  query-kind marker `Q` and the canonical semantic-argument encoding it passes
  to `QueryIdentity::compose`.
- **TR02.** A native fact's basis is the `InputBasisId` of the captured view
  the request resolved against. A TypeScript-authoritative fact's basis is the
  `PublishSnapshot::input_basis` its binding certified. A composed result has
  no basis field (`TR23`). The ordered child bases are the sequence of child
  results, and each child carries the one `InputBasisId` it was observed
  under. That sequence is not an `InputBasisId`, so it cannot fill
  `SemanticFlightKey.input_basis`, and nothing derives or coerces it into one.
  The composed result mints no basis and no flight key. TypeInfo defines no
  sum, list, alias or wrapper to hold the sequence (`TR01`, `TR28`). Canonical
  order of the children is `TR16`. `TypeObservationBasis` names this role and
  is not a new type (`TIF-F01`).
- **TR03.** Execution limits never enter `QueryIdentity`, `ResultContractId`
  or `SemanticFlightKey`: `ExecutionPolicy` and the SKR-READS `BudgetProfile`.
  A budget changes only the identity of a refusal (DEM0 `DR19`); it never
  produces a different value labelled Complete.
- **TR04.** Presentation never enters `QueryIdentity`: display policy,
  qualification, branding and display budgets are `PresentationProfileId`.
  Wire shape never enters it: schema version, string-table layout,
  graph-export layout and whether already-recorded provenance or diagnostics
  are serialized are `SerializationProfileId`. A requester coordinate
  encoding enters no identity (ENC0 `R08`).
- **TR05.** TypeScript interpretation enters `QueryIdentity` only as the
  `TypeScriptSemanticProfileId` among the observed profiles. Backend, process
  and provider identity (`EngineIdentity`, `ProviderEpoch`, serving lease,
  reported version) never enter it (VID0 `R14`); they are value-side
  provenance (`TR26`).

### Selectors and bases (subblock 2)

- **TR06.** The selector set is closed: `Position`, `FileName`,
  `ProjectName`, `WorkspaceName`, `Component` and `NodeRef`. `Component` is a
  `FileName` plus an export name and a typed `FrameworkProfileId`; its facets
  are TIF1D's. `NodeRef` addresses a node of an earlier fact result whose
  basis is one `InputBasisId`, by that result's `(QueryIdentity,
  InputBasisId)` and the node's decl-slot identity, never by opaque bytes. A
  composed result is not such a parent: it has no `InputBasisId` (`TR02`). A
  follow-on query names the child fact, using that child's own
  `(QueryIdentity, InputBasisId)` and the node's decl-slot. A `NodeRef` that
  names the composition container, or that derives or coerces the ordered
  child bases into an `InputBasisId`, is a typed request error before
  execution. Selectors are decoded into typed identities at the adapter; an
  undecodable selector is a typed request error before execution.
- **TR07.** A `Position` is `(SourceUnitId, SourceByteOffset |
  SourceByteRange)` in `CD1` and always carries a source-revision basis
  `(SourceRevision, ContentId)`. A generated or embedded position reaches
  `CD1` only through its map owner, with the `MapRevision` it was mapped
  through. A requester encoding is converted once at the adapter (ENC0
  `R03`). A position whose basis is not live is refused as `StaleSelector`;
  it is never re-mapped, clamped or snapped (ENC0 `R06`, `R09`).
- **TR08.** A `FileName` is a `SourceUnitId` plus a declaration name and
  namespace (type, value or namespace) or an export name. The adapter decodes
  a path through the registered source authority; an unknown path is a typed
  `UnknownSource` error, never a guessed unit and never a scratch file.
- **TR09.** A `ProjectName` is a `ConfiguredProjectId` plus a module
  specifier and an export name, resolved through that project's own module
  resolution. A request that names a source without a project uses the one
  provider-neutral default-configured-owner decision of the workspace
  snapshot; nothing picks the first project that answers.
- **TR10.** A `WorkspaceName` is a name across the workspace. The workspace
  index supplies candidate declarations only; it never answers a type fact and
  never acts as a checker. Each candidate is answered on demand as its own
  `FileName` or `ProjectName` observation. More than one candidate is
  `Ambiguous { candidates }`. IDX0 implements the candidate source
  (`IDX0-AC2`).
- **TR11.** Every selector carries exactly one basis: `SourceRevision` (a
  position, or a name pinned to one revision) or `CapturedView` (the
  `InputBasisId` of a captured project or workspace view). A request may ask
  for the current view; the host captures it and reports that
  `InputBasisId`. A `NodeRef`'s basis is the `InputBasisId` of the fact result
  it names (`TR06`); it is never an ordered list of child bases. A pinned
  basis that is no longer live is `StaleBasis`. An input the basis needs and
  does not hold is `NeedInputs`, never absence (CFG0 `CR23`).
- **TR12.** Name matching is exact on the canonical name: no prefix, fuzzy,
  case-folded or first-match search. Same-name merged declarations are one
  symbol (the `MergedDecl` carrier). The same name in two namespaces, or
  declared by two owners of one file, is `Ambiguous` unless the selector
  names the namespace or owner.

### Operation descriptors and canonical equality (subblock 3)

- **TR13.** The descriptor set is closed (`OP01`–`OP12`, below). A new
  operation is a new row with its own route, contract and owner. IDE features
  that are not type information (references, rename, completion, semantic
  tokens, highlights, code actions) stay on their LSP provider routes; a
  TypeInfo request never takes the shape of the `TypeProvider` feature menu.
- **TR14.** A descriptor names the operation kind, the selector kinds it
  accepts, its semantic arguments (projection mode, reduction demand, closure
  policy, path, substitutions, type arguments, the canonical encoding of a
  structured expression), its `ResultContractId`, the capability cell DEM0
  `P4` demands (VID0 `I10`), its route class and its execution owner. Only an
  argument that can change the answer is a semantic argument.
- **TR15.** Canonical equality material is
  `QueryIdentity::compose(descriptor tag, digest(canonical selector subject +
  semantic arguments), observed profiles, ResultContractId)`. The basis is not
  in it. It enters a `SemanticFlightKey` only when the fact's basis is one
  `InputBasisId` (`TR28`), and it always enters value-side provenance. A
  composed request's `QueryIdentity` is `TR21`; it has no
  `SemanticFlightKey`, and its cache scope is `TR32`. Requests that differ
  only in presentation, serialization, execution limits or requester encoding
  have equal `QueryIdentity`.
- **TR16.** Canonical semantic equality is separate from presentation order.
  Facts, candidates and members are ordered by canonical encoding (union and
  intersection arms by `VerterStableV1`); authored order is a presentation
  projection that changes no equality. The order is the same under any
  file-load, interning, query or thread schedule.
- **TR17.** Every request is validated before semantic execution, as the
  typeinfo wire contract already requires. An unknown descriptor or selector,
  a selector the descriptor does not accept, or an out-of-range argument is a
  typed request error.

| Op | Operation | Selectors | Region claim | Route class | Execution owner | Served at the head |
| -- | --------- | --------- | ------------ | ----------- | --------------- | ------------------ |
| `OP01` | `ResolveSymbol` | FileName, ProjectName, WorkspaceName | whole operation | Native | `ProjectSemanticDispatch` | yes |
| `OP02` | `ProjectPath` | FileName, ProjectName, NodeRef | whole operation | Native | `ProjectSemanticDispatch` | validated, no executor |
| `OP03` | `EvaluateTypeExpression` | FileName | whole operation | Native | `ProjectSemanticDispatch` | serde entry only |
| `OP04` | `ExpandAround` | NodeRef | whole operation | Native | `ProjectSemanticDispatch` | validated, no executor |
| `OP05` | `FlowNarrowingAt` | Position | whole operation | Native | `ProjectSemanticDispatch` (flow substrate) | validated, no executor |
| `OP06` | `ContextualTypeAt` | Position | whole operation | Native | `ProjectSemanticDispatch` | validated, no executor |
| `OP07` | `Relate` | NodeRef, FileName | whole operation | Native | `ProjectSemanticDispatch` (relation oracle) | rejected as `MalformedPayload` |
| `OP08` | `ListSymbols` | FileName | shallow symbol inventory | Native | `IndexedReady` shallow inventory | `list_file_symbols` |
| `OP09` | `ShallowSurface` | FileName, NodeRef | whole operation | Native | `ProjectSemanticDispatch` | `resolve_shallow_surface` |
| `OP10` | `ComponentFacets` | Component | no requested facet needs a TypeScript fact | Native | `ProjectSemanticDispatch` | `resolve_framework_surface_with_audit` |
| `OP10` | `ComponentFacets` | Component | at least one requested facet needs a TypeScript fact | Composed | TIF1; executes no child | native child served; TypeScript child unserved |
| `OP10` child | framework facet | Component | Native child of that composed claim | Native | `ProjectSemanticDispatch` | served with the facet |
| `OP10` child | path-mapped member type | Component | TypeScript child of that composed claim | TypeScriptAuthoritative | `EngineBackend` over `CertifiedTypeEngineBinding` | unserved |
| `OP11` | `TypeAtPosition` | Position | position that is not a framework-bound template position | TypeScriptAuthoritative | `EngineBackend` over the binding | no; LSP reads `get_hover` |
| `OP11` | `TypeAtPosition` | Position | framework-bound template position | Composed | TIF1; executes no child | no |
| `OP11` child | framework binding | Position | Native child of that composed claim | Native | `ProjectSemanticDispatch` | no TypeInfo route |
| `OP11` child | type at the position | Position | TypeScript child of that composed claim | TypeScriptAuthoritative | `EngineBackend` over the binding | no; LSP reads `get_hover` |
| `OP12` | `DeclarationAtPosition` | Position | position that is not a framework template position | TypeScriptAuthoritative | `EngineBackend` over the binding | no; LSP reads `get_definition` |
| `OP12` | `DeclarationAtPosition` | Position | framework template position | Composed | TIF1; executes no child | no |
| `OP12` child | framework binding | Position | Native child of that composed claim | Native | `ProjectSemanticDispatch` | no TypeInfo route |
| `OP12` child | declaration | Position | TypeScript child of that composed claim | TypeScriptAuthoritative | `EngineBackend` over the binding | no; LSP reads `get_definition` |

TIF1D owns the `OP10` facet schema (`TIF1D-AC2`). That schema is not an
execution owner. A framework-bound template position is a framework template
position whose binding is a framework fact. A framework template position is
a projected template position (`TR07`); projection alone does not select
`Composed`. The claims of one `(descriptor, selector kind)` are the partition
`TR18` states, so each request matches one region, and that region and every
composed child have one route class and one execution owner. The
implementation node for each is TIF1 (`TIF1-AC2`).

### Owner-routed plans (subblock 4)

- **TR18.** Each `(descriptor, selector kind, region claim)` has exactly one
  route class: `Native`, `TypeScriptAuthoritative` or `Composed`. The route
  class decides the execution owner. Nothing tries one owner and falls back
  to another. The region claims of one `(descriptor, selector kind)` are a
  partition of the inputs that descriptor accepts for that selector: the
  predicates are pairwise disjoint, their union is those inputs, and a
  request matches exactly one claim. Child rows are the children of the one
  `Composed` claim that matched; they are not peer claims. UAO0's ownership
  validator (`UAO0-AC-R1`) checks that partition. The predicates are:
  - `OP01`–`OP09` have one claim covering the whole operation (`OP08`'s claim
    is the shallow symbol inventory). The partition is that single claim.
  - `OP10`: `no requested facet needs a TypeScript fact` → `Native`;
    `at least one requested facet needs a TypeScript fact` → `Composed`.
    The two are complements over a `ComponentFacets` request. A macro or
    declaration facet whose member type needs a TypeScript fact matches only
    the second; its native facet is that plan's `Native` child. The facets
    the `Native` claim answers are macro and declaration facets.
  - `OP11`: `position that is not a framework-bound template position` →
    `TypeScriptAuthoritative`; `framework-bound template position` →
    `Composed`. The two are complements over `Position`. A script position,
    and a projected template position whose binding is not a framework fact,
    match only the first. A Vue template position inside a configured project
    whose binding is a framework fact matches only the second. Absence of a
    configured project is `Unavailable { NoProject }` on the first route
    (`TR20`), not a second claim.
  - `OP12`: `position that is not a framework template position` →
    `TypeScriptAuthoritative`; `framework template position` → `Composed`.
    The two are complements over `Position`.
- **TR19.** A `Native` route that answers a fact Verter's engine owns — a
  declaration lowered to typed IR, a macro payload, or a framework facet —
  executes only through `ProjectSemanticDispatch`, the one type-resolution
  engine. The parentheticals on `OP05` (flow substrate) and `OP07` (relation
  oracle) name that engine's substrate. They are the same owner and the same
  route class. A `Native` route that answers the shallow symbol inventory
  (`OP08` `ListSymbols`) reads the `IndexedReady` artifact through the session
  and does not issue a `ProjectSemanticDispatch` query. `OP08`'s one
  execution owner is that `IndexedReady` read.
- **TR20.** A `TypeScriptAuthoritative` route executes only through
  `EngineBackend` over a `CertifiedTypeEngineBinding` reached from a
  `BoundProject`. The native engine never recreates such a fact. Without a
  binding the fact is `Unavailable { NoProject | ProviderUnavailable |
  NotReady }`, never a native answer.
- **TR21.** A `Composed` route is an ordered list of child observations, each
  a full observation with its own route class and its own execution owner.
  The composition owner executes no child. Composition is per fact: each fact
  keeps the one authority that produced it. Two authorities answering the
  same question both appear with their authorities; no field-wise winner is
  chosen. The composed `QueryIdentity` is composed over the composition
  descriptor and the child identities. The composed result mints no
  `InputBasisId` and no `SemanticFlightKey` (`TR02`, `TR28`).
- **TR22.** Each `(descriptor, selector kind, region claim)` has exactly one
  route class and exactly one execution owner. A `Composed` region's
  execution owner is its composition owner; it executes no child. Each child
  in that plan has exactly one route class and one execution owner: `Native`
  through the owner `TR19` names, or `TypeScriptAuthoritative` through
  `EngineBackend` (`TR20`). A region missing that shape is a construction
  failure of the descriptor table. A row the host does not serve answers a
  typed `Unsupported` refusal; it is never validated and then dropped.

Because those predicates are complements, every accepted input matches
exactly one region, and that region has exactly one route class and one
execution owner. Every composed child does too, so the charter's abort
condition ("an operation lacks exactly one ratified execution owner") does
not fire. Five rows are validated today without an executor; that is
displaced route `TIF-D06`, not a missing owner.

### Result DTOs (subblock 5)

- **TR23.** A native or TypeScript-authoritative result carries: the typed
  selector echo; the descriptor id; the `QueryIdentity` digest; its one
  `InputBasisId` (`TR02`), plus `required_version`, `content_hash` and
  `map_hash` for a TypeScript-authoritative fact; the facts, each with value,
  authority (`Native`, `TypeScript`, `Framework { FrameworkProfileId }` or
  `Composed`), per-fact quality and provenance (decl-slot identity,
  `SourceUnitId` and `CD1` range, `ReleaseId` for a framework fact);
  candidates or ambiguity; any refusal; the reuse class; and the SKR-READS
  evidence (proof, diagnostic recipes, recovery, cost receipt, footprint).
  A composed result carries those same fields except the basis: that field is
  absent. It reports the ordered child bases only by retaining each child
  result, and each child carries its own `InputBasisId`. TypeInfo defines no
  carrier for that sequence (`TR01`, `TR02`, `TR28`). Explanations and traces
  are optional capture.
- **TR24.** Per-fact quality is `Complete`, `Approximate { cause }` or
  `Unavailable { cause }`, and it lives only in result provenance. A request
  states demand (mode, closure, path), never completeness. A result-level
  completeness, where shown, is derived from the facts, and a cached
  candidate's nominal mode never claims it.
- **TR25.** Quality is independent of reuse eligibility. `ReuseClass`
  (`Shared`, `RequestOnly`, `NoReuse`) is reported beside it; a
  deterministically Approximate fact may be `Shared`. An eligible refusal is
  reused only under its exact refusal identity (DEM0 `DR19`) and stays a
  refusal: it is never read as exact absence, and an edit invalidates it.
- **TR26.** Provider identity is provenance, kept apart from the semantic
  value. No DTO carries a provider handle: no `BoundProject`, binding,
  serving lease, session handle, live pointer, or opaque bytes that
  dereference host or provider state.
- **TR27.** Ambiguity is typed: `Ambiguous { candidates }`, in canonical
  order, each candidate with its own provenance. Cancellation, budget, stale
  basis and unsettled input are typed causes. A miss caused by churn or a
  non-current view is `Unavailable { UnsettledInput }`, never the same signal
  as "no such declaration".

### Caching and invalidation (subblock 6)

- **TR28.** Candidate lookup is keyed by `QueryIdentity`. In-flight production
  of a fact whose basis is one `InputBasisId` is keyed by `SemanticFlightKey`
  `(QueryIdentity, that InputBasisId)`. Both keys belong to the accepted
  runtime owner (the G2 flight law, PER0D). A composed request has no
  `SemanticFlightKey`: it has no basis field, and the ordered child bases are
  the child results' own `InputBasisId`s (`TR02`). TypeInfo mints no basis,
  alias, wrapper or second key to collapse that sequence (`TR01`). Concurrent
  composed requests share work only through their children's flights. The
  composed cache entry is `TR32`,
  not a flight. TypeInfo adds no key, flight, coalescer or store; every cache
  it reads is a `ProjectTypeStore` cache.
- **TR29.** A warm hit requires the value-side `ReadSetSignature` facts and
  self roots to validate against the live store view. A TypeScript-
  authoritative fact also requires its certified basis to be the live
  published basis (`publish_admitted`, `serving_admitted`). A moved backend,
  provider epoch or required version makes the fact
  `Unavailable { StaleBackend }`; it is neither served nor warmed.
- **TR30.** A position-derived fact is valid only for the exact `ContentId`
  and `MapRevision` it was computed on (ENC0 `R09`). A changed map rejects it;
  nothing re-maps it approximately.
- **TR31.** A degraded outcome publishes no fact, no provenance and no
  partial value: no fact entry, no reverse-index metadata and no persistent
  artifact of the value. Required state survives capture off. That
  prohibition is what `ReturnOnly` means for the value.
  An eligible refusal under its exact refusal identity (`TR25`, DEM0 `DR19`)
  is stored and published in a refusal class. That class is not a fact entry,
  and the refusal stays a refusal. A budget-exceeded refusal of an isolated
  root is eligible (`TC11`): the same-profile repeat returns the stored
  refusal and does no producer work.
  These outcomes are not eligible refusals and publish no refusal entry:
  `Unavailable { UnsettledInput }` (a torn or non-current view, `TC12`),
  `Unavailable { StaleBackend }` (the superseded fact is neither served nor
  warmed, `TR29`, `TC05`), cancellation (DEM0 `DR29`) and a superseded epoch
  (DEM0 `DR25`). An Approximate fact caused by budget or cancellation is a
  partial value and is not stored. Only the eligible refusal, when the
  outcome is one, is.
- **TR32.** A composed entry is scoped by the composition descriptor, the
  child identities and each child's basis, and isolated by profile, edit and
  evaluation context. A composed entry never satisfies a child query, and a
  child entry never satisfies a composed one.

## Outcomes and owners

| Outcome | Owner | Receiving acceptance |
| ------- | ----- | -------------------- |
| `TIF-O01` descriptors on the imported identity, no TypeInfo-owned key/flight/basis (`TR01`–`TR05`) | TIF1 | `TIF1-AC2` |
| `TIF-O02` typed Position, FileName, ProjectName, Component and NodeRef selectors with bases (`TR06`–`TR09`, `TR11`, `TR12`) | TIF1 | `TIF1-AC2` |
| `TIF-O03` WorkspaceName candidates from the index, never a type answer (`TR10`) | IDX0 | `IDX0-AC2` |
| `TIF-O04` closed descriptor table with canonical equality and ordering (`TR13`–`TR17`) | TIF1 | `TIF1-AC2` |
| `TIF-O05` owner-routed Native, TypeScript-authoritative and Composed plans (`TR18`–`TR22`) | TIF1 | `TIF1-AC2` |
| `TIF-O06` result DTOs (`TR23`–`TR27`) | TIF1 | `TIF1-AC2` |
| `TIF-O07` caching and invalidation, composed-cache scope (`TR28`–`TR32`) | TIF1 | `TIF1-AC3` |
| `TIF-O08` emit symbol-meaning selectors over this contract | TSF0 | `TSF0-AC1` |
| `TIF-O09` per-operation budget classes and audit events on the descriptors | PER0D | `PER0D-AC2` |
| `TIF-O10` executable ownership validator and fixtures for this decision | UAO0 | `UAO0-AC-R1`, `UAO0-AC-R2` |

TIF1 is the implementation owner of the façade (UAK0 `B06`); its reconciled
contract migrates consumers and public bindings to "the accepted generic
observation identity plus TIF0 operation descriptors". TIF1D builds the
ComponentInfo view on `OP10` and owns the facet schema.

## Consumers

UAK0, DEM0 and ENC0 already record the façade's current consumers
(`C08`–`C12`, `DEM-C02`, `E-C01`). Owner and receiving acceptance stay in
those products. `referencedConsumers` names the product file and copies
neither.
The LSP calls no TypeInfo entry at this head. TIF0 adds the later nodes that
build on this contract:

| Consumer | Reads | Owner |
| -------- | ----- | ----- |
| `TIF-C01` ComponentInfo view and facet schema | `OP10`, `TR06`, `TR23`, `TR24` | TIF1D (`TIF1D-AC2`) |
| `TIF-C02` `defineComponent` template canary, private-harness hover and definition | `OP11`, `OP12`, `TR21` | EAK1 (`EAK1-AC2`) |
| `TIF-C03` `verter typecheck` composed diagnostics with provenance and `NeedInputs` | `TR18`, `TR21`, `TR23`, `TR24` | CLI2 (`CLI2-AC2`) |
| `TIF-C04` GraphQL result and variable type linkage | `OP01`, `OP03`, `TR23` | GQL3 (`GQL3-AC3`) |
| `TIF-C05` security source/sink/sanitizer binding by symbol and version | `OP01`, `OP12`, `TR08`, `TR09` | SEC1 (`SEC1-AC3`) |
| `TIF-C06` DOM and Web API symbol resolution against the active realm | `OP01`, `OP11`, `TR12` | WBC4 (`WBC4-AC3`) |
| `TIF-C07` extension SDK contribution validation | `TR23`, `TR24`, `TR26` | XSDK2 (`XSDK2-AC3`) |
| `TIF-C08` emit symbol-meaning selectors and their read sets | `TR06`, `TR08`, `TR23`, `TR24` | TSF0 (`TSF0-AC2`) |
| `TIF-C09` cache, flight and budget law | `TR03`, `TR15`, `TR28`, `TR31` | PER0D (`PER0D-AC2`) |
| `TIF-C10` workspace index entries as WorkspaceName candidates | `TR10`, `TR16` | IDX0 (`IDX0-AC2`) |

## Displaced routes recorded here

UAK0, VID0, ENC0 and DEM0 do not cover these six. Each has one
production-capable deletion owner, TIF1 (`TIF1-AC1`).

| Route | Unit | Disposition |
| ----- | ---- | ----------- |
| `TIF-D01` | Path-string selectors: every request arm and host entry names its subject by a canonical path string and a bare name (`ResolveSymbolGraphRequest.canonical_id`, `scope_canonical`, `ComponentSelector.canonical_id`, `resolve_named_symbol(canonical_id, name)`, …) | replace with the typed selectors of `TR06`–`TR12` |
| `TIF-D02` | `ExpandGraphAroundRequest` names its parent by `GraphHandle.opaque` bytes | replace with a `NodeRef` over a fact result's `(QueryIdentity, InputBasisId)` and decl-slot (`TR06`); a composed container is not a parent |
| `TIF-D03` | `GraphQueryIdentity` mixes semantic arguments, presentation, serialization, env hashes and versions in one untyped echo; the graph executor fills the env and project fields with empty values and the framework-surface graph has no query | replace with the `QueryIdentity` digest, `PresentationProfileId`, `SerializationProfileId` and basis provenance |
| `TIF-D04` | Five public entry vocabularies beside the graph envelope: `list_file_symbols`, `resolve_named_symbol(_wire)_with_audit`, `evaluate_type_expression_with_audit`, `resolve_shallow_surface(_for)`, and `resolve_symbol_graph_with_audit` (no production caller) | replace with one `TypeInfoRequest` over `OP01`–`OP12` |
| `TIF-D05` | `current_store_view_for_query` reports a non-current view as `None`, the same signal as "could not be resolved" | replace with `Unavailable { UnsettledInput }` |
| `TIF-D06` | Five operations pass validation and reach no executor; `Relate` is rejected as `MalformedPayload` | replace: one execution owner per row, a typed `Unsupported` for unserved rows |

TIF0 references these routes owned elsewhere. Deletion owner and receiving
acceptance stay in the owning product. This table records only the TIF0-side
concern: why the route matters here, and the successor node this contract
names.

| Route | Successor | Why | Owning product |
| ----- | --------- | --- | -------------- |
| UAK0 `D12`, `D13`, `D14` | TIF1 | Component-meta, public-API and off-store framework-surface authorities; `OP10` names the component successor and `TR28` forbids a second store | `tests/kernel/UAK0/products/deletion-retag-ledger.v1.json` |
| UAK0 `D15` | TIF1 | The FFI serde request vocabulary, the FFI half of `TIF-D04` | `tests/kernel/UAK0/products/deletion-retag-ledger.v1.json` |
| UAK0 `D10`, `D11` | COX0 | Per-framework LSP branches and MCP `is_vue()` gates | `tests/kernel/UAK0/products/deletion-retag-ledger.v1.json` |
| DEM0 `DEM-D03` | TIF1 | The executor resolves every surface kind; `OP10` demands requested facets only | `tests/kernel/DEM0/products/demand-inventory.v1.json` |
| VID0 `V-D02` | TIF1 | Open `framework_adapter_id` string; `TR06` makes it a typed profile | `tests/kernel/VID0/products/identity-inventory.v1.json` |
| ENC0 `P19` | ENCF0 | `GraphSpanRef` and `GraphDiagnostic` spans with no stated unit | `tests/kernel/ENC0/products/boundary-route-ledger.v1.json` |

Deletion-category coverage:

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | none: framework surfaces dispatch through the adapter registry, and its open selector string is `V-D02` | `D10`, `D11` |
| untagged coordinate/public identity | `TIF-D01`–`TIF-D03` | `V-D02`, `P19` |
| duplicate component information authority | none: UAK0's deletion ledger owns `D12`–`D15`; this decision does not copy that assignment | `D12`–`D15` |
| broad or untruthful TypeInfo request vocabulary (this decision's own population) | `TIF-D04`–`TIF-D06` | — (`D15`, its FFI half, keeps its UAK0 category) |

`DEM-D03` keeps DEM0's eager-selector category.

The reconciled contract supersedes "broad `TypeProvider`-shaped public
requests after all consumers move". At this head no public TypeInfo request
has that shape: `TypeProvider` and `EngineBackend` are the LSP's internal
provider routes and stay. The broad public shape is the five-entry
vocabulary (`TIF-D04`) and its FFI half (`D15`); `TR13` keeps the provider
feature menu out of the descriptor set.

## Findings recorded for the receiving owners

- **`TypeObservationBasis` has no type, and a composed list is not a flight
  key** (`TIF-F01`, UAO0 `UAO0-AC2`). The accepted basis of a native or
  TypeScript-authoritative fact is `InputBasisId`. A composed result has no
  basis field; the ordered child bases are the child results' own
  `InputBasisId`s. That sequence is not an `InputBasisId`, forms no
  `SemanticFlightKey`, and is not a type TypeInfo defines. Each child flights
  on its own key. `TR02` and `TR28` mint nothing. UAO0 revalidates that no
  downstream node names the role as a type, defines a carrier for the
  sequence, or fills `SemanticFlightKey` from it.
- **The certified engine answer has no payload** (`TIF-F02`, TIF1).
  `EngineBackend::query` returns `Answered` or `NoResultVersionMismatch`; the
  engine-native answer rides an out-of-band channel. `OP11` and `OP12` need a
  typed fact inside `TR23`, without a second backend contract.
- **Multi-owner ambiguity is silent** (`TIF-F03`, TIF1).
  `unique_local_type_declaration_owner_in` returns `None` on multi-owner
  ambiguity and `resolve_named_symbol` falls back to the ordinary-file owner
  without reporting it.
- **Facet normalizers correlate by first name match** (`TIF-F04`, TIF1). The
  Svelte `$bindable` and Vue slot normalizers use `iter().find` by name; the
  Svelte one synthesises a field when nothing matches. This sits in the
  `D12`/`D14` component population, not in a TypeInfo selector.
- **Hover merging is presentation** (`TIF-F05`, EAK1 `EAK1-AC2`).
  `merge_hover` prepends the provider's type block to Verter's hover text.
  When a hover adopts `OP11`, its type and framework facts follow `TR21`.
- **The quality vocabulary maps onto `TR24`** (`TIF-F06`, TIF1). Proposed:
  `EXACT_RESOLVED`, `EXACT_SYMBOLIC` and `UNRESOLVED_GENERIC` are Complete (an
  open generic is an exact answer); `PARTIAL` and `CYCLE` are Approximate;
  `MISS` is `Unavailable { NotFound }` unless the read set proves absence;
  `UNSUPPORTED`, `BUDGET_EXCEEDED` and `UNSTABLE` are Unavailable with that
  cause. A facet's `UNSUPPORTED` is not-applicable for its profile, which
  TIF1D defines.

## Empty populations

- **No TypeScript-authoritative TypeInfo operation.** Every served entry is
  native; TypeScript type facts reach only LSP features. `OP11` and `OP12`
  are new.
- **No composed result.** Nothing composes a framework surface with a
  TypeScript fact (`TIF-F05` composes text). `TR21` and `TR32` displace
  nothing.
- **No project or workspace selector.** `GraphSpanRef` is the only position
  selector, and no executor reads it (ENC0 `P19`).
- **No result basis.** No TypeInfo result carries an `InputBasisId` or a
  revision.
- **No native recreation of TypeScript facts.** The `oracle_core` hover
  harness compares native results with checked-in TypeScript snapshots; its
  generator is behind the `oracle-gen` feature and its consumers are
  `cfg(test)`. `TR20` displaces nothing.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test, CI
check or validator.

- **AC1 — ownership contract.** The inventory binds every subblock rule,
  descriptor, outcome, consumer and displaced route to one existing plan node,
  a successor path from TIF0 and a receiving acceptance ID. UAK0, DEM0, VID0
  and ENC0 rows are referenced, not re-owned. The executable validator is
  UAO0's (`UAO0-AC-R1`); the fixtures `TC01`–`TC08` and `TC11` are UAO0's
  (`UAO0-AC-R2`). TIF1 runs `TC08` for its compatibility projections and owns
  `TC09`, `TC10`, `TC12` and `TC13`. Production deletion of `TIF-D01`–`TIF-D06`
  is `TIF1-AC1`.
- **AC2 — positive contract.** Existing coverage pins the identities,
  provenance, completeness and ordering this contract builds on (the full
  list is `existingCoverage` in the inventory):
  - the imported identity: `query_identity_is_snapshot_independent_and_flight_key_is_basis_bound`,
    `result_contract_is_derived_deterministically_from_the_row` and
    `alias_and_direct_references_share_one_composed_identity` in
    `crates/verter_session/tests/g_extts/semantic_capability_closure.rs`; the
    `input_basis_id_is_not_query_identity` and
    `query_identity_is_not_semantic_flight_key` compile-fail fixtures in
    `verter_identity`;
  - the certified binding and stale basis: `a_superseded_snapshot_is_refused_before_it_can_warm`,
    `a_retained_binding_is_not_admitted_by_a_rotated_serving_session` and
    `the_flight_key_of_a_published_flight_is_basis_bound` in
    `g_extts/certified_engine_seam.rs`; the
    `uncertified_engine_answer_is_unrepresentable` compile-fail case;
  - validation before execution and fail-closed operations: the
    `typeinfo_request_validation` cases, `framework_surface_wire_executor_validates_first`,
    `an_unserved_operation_is_refused_fail_closed` and
    `include_degraded_is_rejected_as_unsupported`;
  - truthful status: `a_degraded_framework_surface_never_encodes_exact_resolved`
    and `supported_empty_kind_is_distinct_from_unsupported` in
    `g_block/framework_surface_executor.rs`;
  - the closed wire: `typeinfo_graph_taxonomy`, `typeinfo_proto_roundtrip`,
    `typeinfo_proto_ts_contract` and the `typeinfo_*_contract_guards`;
  - ordering: `encode_is_deterministic_and_wire_roundtrips` in
    `crates/verter_protocol/tests/cases/typeinfo_graph_export.rs` and
    `signature_kernel_interned_identities_are_schedule_independent`.

  New or extended tests belong to UAO0 and to the owners named per case.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. `TR28`–`TR32` and the cases `TC05`, `TC06`, `TC08`, `TC11`
  and `TC12` bind the later proof to TIF1 (`TIF1-AC3`) and UAO0
  (`UAO0-AC-R2`). Native edit invalidation is already pinned by the
  `cache_invalidation_*` cases in `crates/verter_session/src/typeinfo/typeinfo_tests/cache_invalidation.rs`.
- **AC4 — bounded work: not applicable.** No hot path changes. `TC01` and
  `TC02` fix the zero cross-authority calls that UAO0 proves (`UAO0-AC-R2`);
  per-operation budgets are PER0D's (`TIF-O09`), and the zero-work baseline
  stays UAK0's `Z01`, confirmed by PER0E.
