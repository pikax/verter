# Versioned public requests, results and capability truth

This is the public contract consumed by Rust, FFI, NAPI, WASM, LSP, MCP and
CLI adapters. Today the boundary is owned by framework-shaped host/session
registries and untagged public boundaries. Its final sole authority is the
typed immutable universal catalog and demand-selected kernel services.
Transport adapters translate that authority; they do not infer semantics.

The source inventory describes `docs(meta): define component workbench contract
and evidence boundaries (#880)`, 2026-10-13. The title and date are descriptive
context, not acceptance evidence. This follows the [docs-only rule](README.md):
it adds no production types, tests, validator, generator or CI lane. PUB0T
implements the shared envelope and retires the current adapters' duplicate
outcome authority. UAP0 independently verifies the public contract; VIM1
generates the capability/maturity matrix. These are receiving obligations,
not claims that the target envelope already exists.

## Authority and imported contracts

The [catalog](catalog.md) supplies immutable registration and canonical row
order. The [identity law](identities.md) supplies exact source, release,
carrier, framework, project and catalog identities. A display family, path,
provider handle or editor language string cannot substitute for them.
`CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`,
`DemandPlan` and `TypeInfoRequest` retain the meanings assigned by those
decisions, [demand activation](demand-activation.md) and the
[TypeInfo façade](typeinfo-facade.md). This contract adds no registry or query
key type.

The [coordinate law](coordinates.md) owns conversion and map validity.
[Participation](cox0d-participation.md) owns effective per-cell eligibility;
[cache, flight, cancellation and budget law](per0d-cache-flight-budget-law.md)
owns reuse and delivery. [ComponentInfo](tif1d-componentinfo-view.md) remains
a TypeInfo view, with per-child identities, bases, quality and provenance.
[Rules/actions](rules-actions.md) and [formatting](formatter.md) retain their
operation and edit-safety authority. Public wrapping cannot promote their
results, choose another provider, resolve again or create a cache.

## Request and result contract

The field descriptions below specify semantic roles, not a second DTO schema.
PUB0T owns the concrete shared protocol/session types and generated adapters.

- **PE01 — negotiation.** A request explicitly selects a supported envelope
  schema epoch and operation result contract. The adapter negotiates before
  execution; an unknown epoch or required operation is a typed Unsupported
  outcome with supported versions, never a default interpretation. Envelope,
  payload and catalog/profile epochs remain distinct. Serialization changes
  belong to `SerializationProfileId`, not `QueryIdentity`.
- **PE02 — exact subject.** The request carries the operation's typed selector,
  exact source lineage/revision/content, relevant map revision/domain,
  configured project, captured configuration and observed profile identities,
  catalog identity and
  current demand/publication epoch. It supplies only the identities that the
  owning operation requires. A path is lookup/presentation data, not evidence
  that two subjects, snapshots or projects are interchangeable.
- **PE03 — observation.** Each returned semantic fact retains its producer's
  `QueryIdentity`, `ResultContractId`, `InputBasisId`, profile and provenance.
  A composed container retains the canonically ordered children and each
  child's basis; it invents no combined `InputBasisId` or semantic flight key.
  Backend/process identity is value-side provenance, never a query key.
- **PE04 — quality.** The result carries one typed delivery outcome plus each
  fact's WIRE status and open causes, enumeration coverage, semantic certainty
  and evidence kind. Enumeration is Complete, Partial or Unknown; semantic
  certainty is definite, conditional, possible or unresolved; evidence is
  static, observed or estimated. These dimensions are independent. A complete enumeration
  can contain uncertain facts; a precise fact can belong to a partial
  enumeration. Static facts, execution observations and estimates never share
  an undifferentiated truth label. Capture-disabled detail is Unavailable,
  not an empty trace, zero cost or proof that no work occurred.
- **PE05 — deterministic order.** Canonical typed keys and the operation's
  specified ordering determine facts, candidates, causes and references.
  Provider/admission order, hash iteration and asynchronous completion order
  cannot select an owner or reorder semantic answers. Presentation rank and
  CLI formatting stay outside semantic identity and result equality.
- **PE06 — bounded payload.** Preserve READS leased delivery and the owning
  result's bounded graph/reference representation. Public copies transfer
  their charge exactly once through the declared boundary. No raw engine graph
  ID, provider session/pointer, browser object, debugger variable lifetime or
  test-runner state enters a semantic snapshot. A typed handle names its sole
  owner's object, scope, schema, lease and freshness; it is not an engine ID.
- **PE07 — propagation.** Cancellation and budget belong to execution policy,
  not semantic keys. Every adapter forwards cancellation, localized refusal
  causes and current delivery eligibility. Stale, cancelled, refused and
  incomplete deliveries cannot warm a complete-result cache or acquire current
  publication authority. Presentation limits cannot relabel a truncated answer
  Complete. Limits retain the producer's units and scope.
- **PE08 — coordinates.** Internal positions are checked UTF-8 byte offsets in
  a named authored/generated domain. A client encoding is negotiated at its
  adapter and converted once against the exact content/map snapshot. Public
  positions name their unit/domain; invalid boundaries or stale maps refuse
  instead of clamping, guessing or silently using UTF-16.
- **PE09 — prepared inputs.** Core requests consume the producer's immutable
  prepared inputs and exact dependency identities. Filesystem acquisition,
  external engines and process handles belong to the host. WASM lacking a
  required declaration, project, map or dependency returns typed NeedInputs
  identifying the missing input; it cannot read a hidden filesystem or report
  an empty successful result. Recovery submits a new current request after
  preparing those inputs; it does not patch an old snapshot in place.
- **PE10 — compatibility.** Retain protobuf reserved fields/tags and never
  reuse a removed tag or change its meaning. Additive optional fields may be
  ignored only when they cannot alter required interpretation or authority.
  Unknown required variants fail negotiation; omitted required metadata is
  malformed, never a default success. Thin transport wrappers may carry request
  correlation, presentation and audit references, but define no semantic
  outcomes of their own. Retire duplicate envelopes only after real generated
  consumer parity within PUB0T's candidate; no dual-running authority remains.

## Outcome inventory

Each row has one implementation owner. `successorPath` for every row is
`PUB0 → PUB0T`; receiving acceptance is `PUB0T-AC1`, with lifecycle enforcement
under `PUB0T-AC4`. This inventory includes errors as well as successful values.

| ID   | Outcome                    | Required meaning; forbidden conversion                                                                                                                                 |
| ---- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| PO01 | Success                    | Complete for the requested contract and current eligibility; a meaningful empty set is allowed only with proved complete enumeration, never as missing-input recovery. |
| PO02 | Partial                    | Usable facts with explicit missing coverage, per-fact quality and localized causes; no Complete promotion or lost open cause.                                          |
| PO03 | Ambiguous                  | Canonically ordered competing claims/candidates and their provenance; no first-loaded or fastest-returning winner.                                                     |
| PO04 | NeedInputs (`need_inputs`) | Typed unresolved prepared-input requirements and recovery scope; not an empty success or Unsupported substitute.                                                       |
| PO05 | Unsupported                | Unsupported schema/operation/client or execution capability, with the specific reason; registration is not proof of support.                                           |
| PO06 | NotApplicable              | Proven operation/profile/selector inapplicability, distinct from unproven applicability, Disabled participation or unavailable provider.                               |
| PO07 | Cancelled                  | Request/delivery cancellation, preserving its cause and any explicitly return-only detail; no serve, warm or publish.                                                  |
| PO08 | Stale (`stale`)            | Subject, profile, demand epoch, lease or snapshot no longer eligible; recovery requires a current request.                                                             |
| PO09 | Refused (`refused`)        | Typed malformed/invalid request, budget, authority or safety refusal; preserve localized cause and recovery, never stringify into Success.                             |
| PO10 | Unavailable                | Applicable supported operation lacks a ready execution dependency or captured detail; distinguish NoProject, ProviderUnavailable, NotReady and capture-disabled.       |

`QueryOutcome` and existing TypeInfo error arms are evidence of current native
delivery, not a claim that they already implement this whole table. PUB0T
maps native and TypeScript-authoritative deliveries without losing their
causes. Agent spelling is an adapter mapping onto the same outcomes, not an
agent-only semantic enum. Agent `complete` maps to Success. Agent `pending`
names a still-running execution handle with no semantic value yet; it is
transport execution state, not Complete, Partial or a cacheable semantic fact.
An explicit disabled-participation reason remains
distinct from a supported provider being unavailable.

## Capability and exposure contract

**PE11.** A capability cell is keyed by the exact profile/release, operation,
selector domain and consumer surface. It records applicability, producer
support, maturity, effective participation, client support, execution class,
input requirements and current availability separately. No boolean can stand
in for the cell. COX0 computes eligibility from the existing catalog and demand
plan; VIM1 renders it without becoming a second admission registry. Registration
does not imply applicability, maturity or availability.

**PE12.** Execution classes are Portable (including browser-local execution),
NativeOnly and ExternalOwner (external execution), as specified by DX0.
A browser cannot promote a NativeOnly cell because desktop tests passed.
Native syntax inspection is not a replacement for a missing external strict
TypeScript answer. LSP registers/publishes only applicable Full-participation
cells with actual client support. WorkspaceOnly bounded caller results do not
grant interactive publication, and Disabled cells perform zero producer work.
Static-client registration limitations remain explicit; the server still
rejects ineligible requests. Capability withdrawal and profile/edit healing
retain COX0's epoch law.

**PE13.** DX1 exposure descriptors are metadata on the existing canonical
capability row: exact producer/operation, versioned request/result schemas,
source links, fixture references, execution class, available surfaces, maturity
and RequiredCurrent policy, input requirements, completeness/quality vocabulary
and an executable
inspection/replay path. Producer support promotion requires such evidence,
using a generic structured inspector when no specialized panel exists. A
descriptor alone is not executable coverage or product-terminal acceptance.
PUB0 implements no UI. VIM1's generated matrix consumes this metadata and
checks freshness under `VIM1-AC-R2`.

**PE14.** Versioned capability-tagged references cover diagnostic traces,
runtime/build observations, compatibility evidence, dependency impacts and
edit/migration plans. Each reference retains exact source/project/engine,
snapshot, configuration/profile and execution-host provenance, completeness,
evidence kind and producing-owner identity. The shared
envelope owns reference transport only; payload authority and execution stay
with the product owner. Bounds, lease/freshness and unavailable capture are
explicit. An edit-plan reference grants neither edit permission nor execution.

**PE15.** Agent-facing snapshot, target, result, overlay, plan (with content
digest) and execution handles are distinct typed kinds. AGM2 owns their
envelope rendering and pagination, AGM-ID their text codec, and AGM-HANDLES
the opaque-handle store, lease and release lifecycle. Resource owners retain
resource semantics. Handles preserve authorization scope and never enter
internal semantic query keys. Fields
distinguish `untrusted_text`, `identifier` and `third_party` messages; imported
text is data, not authority or an instruction. AGT0 owns the agent capability
and trust contract; AGT6B owns execution grants, root/handle authorization
and per-call budgets. Neither
creates another public result vocabulary.

## Consumer and implementation ownership inventory

Each row assigns only its public-boundary outcome, leaving producer semantics
with the linked owning contracts. Paths are concrete successor paths, not
speculative implementation sequencing. Existing and later consumers are
separated so a later product terminal cannot become a prerequisite here.

| ID   | Consumer / receiving surface                   | Concrete boundary                                                                                               | Sole implementation owner | Successor path                             | Receiving acceptance         |
| ---- | ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------- | ------------------------------------------ | ---------------------------- |
| PC01 | Rust host/session consumers                    | `verter_session/src/typeinfo/{types,request_validation,graph_query}.rs`, host compile/update/diagnostic results | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC1, AC3               |
| PC02 | FFI transit                                    | `verter_protocol/src/{types,typeinfo}.rs`, `verter_ffi/src/convert/{typeinfo,output,error}.rs`                  | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC2, AC3               |
| PC03 | NAPI / native TS consumers                     | `verter_napi/src/{lib,typeinfo}.rs`, `packages/proto/src/{typeinfo.ts,gen/verter/v1/typeinfo_pb.ts}`            | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC2, AC3               |
| PC04 | WASM / browser TS consumers                    | `verter_wasm/src/{lib,typeinfo}.rs`; prepared-input refusal through actual exports                              | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC2, AC4               |
| PC05 | LSP consumers                                  | `verter_lsp/src/capabilities.rs` and feature/result adapters; COX0 retains per-cell eligibility                 | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC2, AC3               |
| PC06 | MCP consumers                                  | `verter_mcp/src/server.rs` and `tools/framework_surface.rs` and tool result projection                          | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC2, AC3               |
| PC07 | Existing CLI consumers                         | `verter_tsc/src` diagnostic/report adapters; later CLI presentation is CLI1's                                   | PUB0T                     | PUB0 → PUB0T                               | PUB0T-AC2, AC3               |
| PC08 | Public-contract lock                           | Complete inventory validation; real Rust/FFI/NAPI/WASM/TS differential and lifecycle fixtures                   | UAP0                      | PUB0 → UAP0                                | UAP0-AC-R1, AC-R2            |
| PC09 | Generated matrix consumers: docs, IDEs, agents | Per-surface capability/maturity generator and freshness                                                         | VIM1                      | PUB0 → VIM1                                | VIM1-AC-R2                   |
| PC10 | Exposure consumers                             | Descriptor metadata and executable inspection/replay promotion contract                                         | DX1                       | PUB0 → DX1                                 | DX1-AC1, AC2                 |
| PC11 | Agent contract consumers                       | Trust, capability and field-kind interpretation                                                                 | AGT0                      | PUB0 → AGT0                                | AGT0-AC1, AC2                |
| PC12 | Agent handles/pagination consumers             | Envelope rendering of typed snapshot/target/result/overlay/plan/execution references                            | AGM2                      | PUB0 → PUB0T → AGM2                        | AGM2-AC1, AC2, AC3, AC4, AC5 |
| PC13 | Agent execution consumers                      | Execution grants, roots, handle authorization and per-call budgets                                              | AGT6B                     | PUB0 → AGT0 → AGT6B                        | AGT6B-AC1, AC2, AC3, AC5     |
| PC14 | Agent identifier/handle codec consumers        | Typed version/family validation and canonical text spelling                                                     | AGM-ID                    | PUB0 → PUB0T → AGM1 → AGM-ID               | AGM-ID-AC1, AC2, AC3         |
| PC15 | Agent handle lifecycle consumers               | Opaque handle issuance/lookup/release with basis, scope and lease                                               | AGM-HANDLES               | PUB0 → PUB0T → AGM1 → AGM-ID → AGM-HANDLES | AGM-HANDLES-AC1, AC2, AC4    |

PUB0T-AC5 explicitly receives the downstream interface obligation for UAP0,
VIM1, CTX1, EPR5, FMT4P, FMTH0, FWD1, HWCI0, HWCP0, LNT3, LSO6, LSO7,
NCF-MP-PROJECT, NCK2, NCK6, NCK7, SXC6P, CLI1, AGM1 and AGM2. These
consumers use that same envelope; they own their operations, not a new semantic
DTO. The product-reference payloads in PE14 remain outside this generic
boundary's migration population. Existing compile identities, artifacts,
maps and diagnostics remain owned by their producers; wrapping their delivery
does not retire their distinct payload types.

## Displaced-route inventory and deletion disposition

Every new row below has exactly one production-capable retirement owner,
PUB0T, reached by `PUB0 → PUB0T`; receiving acceptance is `PUB0T-AC3`.
Retirement means replacing duplicate _outcome authority_, not deleting a
producer's operation-specific payload or required failure distinction.

| ID   | Current production route                                                                                                                                                                                                                                                         | Required retirement / rejection                                                                                                                          |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| PD01 | `TypeInfoGraphRequest` / `TypeInfoGraphResponse` and `TypeInfoRequestError` in `verter_protocol/proto/verter/v1/typeinfo.proto`, graph helpers in `src/typeinfo/graph.rs`                                                                                                        | Generate the shared negotiated outcome boundary; retain payloads, reserved tags and typed causes, reject adapter bypass.                                 |
| PD02 | Host symbol/resolve/evaluate/shallow/macro entry DTOs in `verter_session/src/typeinfo/{types,surface}.rs` and wire request validation                                                                                                                                            | One shared outcome creation/propagation path; eliminate unqualified optional-value/dispatch-error interpretations after parity.                          |
| PD03 | `NapiTypeInfoResolveResult` and graph result wrappers in `verter_napi/src/typeinfo.rs`, exports in `src/lib.rs`                                                                                                                                                                  | Thin generated NAPI transport; no nullable `typeExpr` or string error becomes semantic truth.                                                            |
| PD04 | WASM resolve and graph wrappers in `verter_wasm/src/typeinfo.rs`, exports in `src/lib.rs`                                                                                                                                                                                        | Thin shared-result transport; missing prepared inputs become NeedInputs, never successful empty JSON.                                                    |
| PD05 | FFI conversions in `verter_ffi/src/convert/typeinfo.rs` and result/error conversions                                                                                                                                                                                             | Preserve shared outcome/cause, coordinate tags and copy charge; delete stale serializers and independent semantic fallback.                              |
| PD06 | MCP `project_response` / `project_error_kind` in `tools/framework_surface.rs` and TypeInfo tool projection                                                                                                                                                                       | Presentation wraps the canonical outcome; no semantic decisions from JSON shape, display strings or missing arms.                                        |
| PD07 | LSP feature handlers and capability advertisement in `verter_lsp/src`                                                                                                                                                                                                            | Consume shared outcome and COX0 eligibility; reject stale-to-success or partial-to-complete conversion, preserving LSP transport conventions.            |
| PD08 | CLI diagnostics/report paths in `verter_tsc/src`                                                                                                                                                                                                                                 | Consume shared delivery truth; exit/text formatting cannot erase partial, stale or refused semantics.                                                    |
| PD09 | Host compile/update/diagnostic delivery through `FfiUpdateResult`, `FfiDiagnosticsSnapshot`, `FfiVirtualFileResponse`, `FfiIdeResponse`, `FfiTscResponse`, `FfiPublicApiResult`, `FfiCrossFileResult`, `FfiRemoveResult` in `verter_protocol/src/types.rs` and output converters | Inventory each delivery adapter before cutover and route outcomes through the shared authority; retain compile-product and diagnostic payload ownership. |
| PD10 | Lint/action/document/selector deliveries through `FfiCodeAction`, `FfiDocumentSymbol`, `FfiSelectorMatchResult` in `verter_protocol/src/types.rs`, `verter_ffi/src/convert/{lint,actions}.rs`, and corresponding host/NAPI/WASM/LSP/MCP entry points                             | Route shared delivery outcomes through the envelope; retain rule applicability, edit safety, symbol and selector payloads with their producer owners.    |

The charter's three deletion categories also include already-owned routes.
These references retain their original owners and successor paths in the
linked products; they are not assigned again or moved into PUB0T:

| Category                         | Owning inventory and concrete routes                                                                                                                                  | Existing sole deletion owner / receiving acceptance                             |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Central framework switch         | [UAK0](authority-inventory.md) D01–D07: grammar/request/capability enums, parse switches, hard-coded registration; CAT-D01–D04 registry/mirror constructors           | CPF1 / CPF1-AC1                                                                 |
| Central framework switch / gates | UAK0 D10–D11 and [catalog](catalog.md) CAT-D06; adapter-id gate and active-provider selection                                                                         | COX0 / COX0-AC1                                                                 |
| Untagged public identity         | UAK0 D08 language/adapter identity; VID0 V-D04 producer-specific source minting                                                                                       | CPF1 / CPF1-AC1                                                                 |
| Untagged public identity         | UAK0 D09 capability gates; VID0 V-D01/V-D05 release inference; V-D02 selector adapter string; V-D06/D19 project key                                                   | COX0 / COX0-AC1; FWA1 / FWA1-AC1; TIF1 / TIF1-AC1; PM1 / PM1-AC-R1 respectively |
| Untagged coordinates             | [ENC0](coordinates.md) boundary-route ledger, inherited D16 non-editor wires and D17 editor wires                                                                     | ENCF0 / ENCF0-AC1 and ENCL0 / ENCL0-AC1 respectively                            |
| Duplicate component information  | UAK0 D12–D15: component-meta authority, public-API projection, row-owned surface cache, legacy serde TypeInfo DTOs; [ComponentInfo view](tif1d-componentinfo-view.md) | TIF1 / TIF1-AC1                                                                 |
| Duplicate component information  | UAK0 D18 per-request component scan                                                                                                                                   | IDX0 / IDX0-AC1                                                                 |

Coordinate conversion, native graph evaluation and TypeInfo view construction
are not duplicate public envelopes. PUB0T consumes their implemented results;
it cannot claim their deletion acceptance. New public DTOs with engine IDs,
provider handles or independently inferred success are forbidden routes and
are rejected under PUB0T-AC3/AC4.

## Receiving case table

These are reviewed contract cases, not new executable tests. PUB0T implements
production parity/lifecycle tests under AC1–AC4; UAP0 independently executes
the public boundary cases under AC-R2. VIM1 alone executes matrix freshness.

| ID   | Input / affected and unaffected controls                                                      | Required result / discriminating regression                                                                                                              | Execution owner / acceptance                                       |
| ---- | --------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| PK01 | Same complete fact through Rust, FFI, NAPI, WASM, LSP, MCP, CLI                               | Same identities, bases, provenance, ordered facts and causes; meaningful empty Complete control survives. Drop a cause or identity and parity fails.     | PUB0T-AC2; UAP0-AC-R2                                              |
| PK02 | WASM missing one prepared dependency; same request with prepared dependency                   | NeedInputs with typed missing key versus complete control. Plant missing-input → empty Success and fail.                                                 | PUB0T-AC2; UAP0-AC-R2                                              |
| PK03 | Partial fact beside unaffected complete child                                                 | Preserve per-fact status, open causes and independent enumeration/certainty/evidence. Dropping a cause or promoting the container fails.                 | PUB0T-AC1, AC4; UAP0-AC-R2                                         |
| PK04 | Competing applicable claims in reversed registration/completion order                         | Same Ambiguous candidates and provenance; no winner by timing. Unambiguous control stays Complete.                                                       | PUB0T-AC1; UAP0-AC-R2                                              |
| PK05 | Old source/map/profile/demand snapshot or expired handle; current control                     | Stale/refused old delivery, current answer unchanged. Plant stale → Success and fail; old work cannot warm/publish.                                      | PUB0T-AC2, AC4; UAP0-AC-R2                                         |
| PK06 | Cancel/budget one request; unrelated current request                                          | Preserve Cancelled/Refused cause and charge ownership; unrelated supported answer unchanged, no warm from degraded delivery.                             | PUB0T-AC4; UAP0-AC-R2                                              |
| PK07 | Unsupported epoch/required variant; supported epoch; reserved tag                             | Typed negotiation refusal, successful supported control, no reserved-tag reuse/default interpretation.                                                   | PUB0T-AC1, AC4; UAP0-AC-R2                                         |
| PK08 | Same Unicode subject with negotiated UTF-8/16/32; stale map and invalid byte boundary         | Equivalent authored range on valid controls; explicit refusal on invalid/stale conversion, no implicit encoding.                                         | PUB0T-AC4; UAP0-AC-R2; coordinate behavior remains ENCF0/ENCL0's   |
| PK09 | Full, WorkspaceOnly, Disabled, inapplicable and unsupported-client cells                      | Only applicable Full/client-supported LSP capabilities publish; zero work for rejected cells, unrelated Full controls remain eligible.                   | UAP0-AC-R2, consuming COX0-AC2/AC3                                 |
| PK10 | Portable/browser, NativeOnly, ExternalOwner with/without ready provider                       | Distinct support/maturity/availability cells; no browser promotion from desktop or native substitution for external strict facts.                        | VIM1-AC-R2; UAP0-AC-R2                                             |
| PK11 | Same facts with capture on/off, bounded copies and trace/reference handles                    | Same semantic answers; capture off detail unavailable, copy charge transferred once, stale handles refuse.                                               | PUB0T-AC4; UAP0-AC-R2                                              |
| PK12 | Exposure descriptor with/without executable replay; edit-plan and observation references      | No unsupported promotion, no execution from a reference, estimates remain separate from observations, exact source/project/engine/completeness retained. | DX1-AC1/AC2; PUB0T-AC4                                             |
| PK13 | Agent overlay/target/plan handles and untrusted third-party text                              | Handle kinds/scopes cannot alias; plan digest retained without granting execution, text cannot become instructions or semantic query identity.           | AGM2-AC1/AC2; AGM-ID-AC3; AGM-HANDLES-AC1/AC2; AGT0-AC2; AGT6B-AC3 |
| PK14 | Inventory missing a surface/outcome, unknown/pathless/conflicting or docs-only deletion owner | Reject malformed ownership data, including valid-looking non-production deletion owners; valid complete inventory accepted.                              | UAP0-AC-R1                                                         |
| PK15 | Matrix semantic row changes; equivalent reordered input; hand-edited mirror                   | Deterministic generated rows, semantic changes reflected, stale mirrors rejected without a second capability authority.                                  | VIM1-AC-R2                                                         |

## Evidence selection and acceptance limits

**PUB0-AC1.** The outcome, consumer, displaced-route and receiving-case tables
are the complete public-boundary inventory for this decision. Inherited route
populations remain in their owning products. UAP0-AC-R1 owns executable
completeness, known owner, successor-path, receiving-acceptance and conflict
validation; PUB0 adds none. PUB0T-AC3 inventories concrete handlers within the
listed adapter families again before production deletion, reconciling source
drift without narrowing this population. The syntax, compile, analysis,
TypeInfo/ComponentInfo, diagnostics, lint/actions and formatter families exhaust
the currently registered service categories; producer payload variants are
not additional delivery authorities. There is no current agent overlay/plan
execution handle population in these core DTOs: PE15 is reserved contract
vocabulary, with the later consumers PC11–PC15 owning its implementation.

**PUB0-AC2.** Existing bounded evidence discriminates the current boundary:
`type_info_graph_request_roundtrip_covers_every_payload_arm`,
`type_info_request_error_roundtrip_covers_every_variant`,
`capability_handshake_roundtrips`,
`semantic_type_graph_roundtrip_preserves_envelope_shape` and
`type_info_graph_response_roundtrip_covers_every_kind_arm` in
`crates/verter_protocol/tests/cases/typeinfo_proto_roundtrip.rs`; schema-version
agreement and message coverage in `typeinfo_proto_ts_contract.rs`;
`identical_inputs_share_parse_keys_and_parse_affecting_changes_do_not` in
`crates/verter_language/tests/cases/parse_identity.rs`; and dispatch fault
classification in `crates/verter_session/src/typeinfo/tests.rs` (budget,
unstable-state, unsupported-intrinsic and alias-cycle cases). These prove
existing serialization, identity and refusal distinctions, not the future
universal outcome or real cross-surface parity. PK01–PK11 bind the missing
production and independent proof to PUB0T and UAP0 explicitly.

**PUB0-AC3 — not applicable to this mutation.** It changes no cache, flight,
cancellation, partial-result or publication authority. PER0D's law remains
unchanged; PUB0T-AC4 and UAP0-AC-R2 receive edited-versus-fresh, profile healing,
out-of-order cancellation and no-warm degraded-delivery proof.

**PUB0-AC4 — not applicable to this mutation.** It changes no executable hot
path, allocations, copies, retained candidates or work counters. No counter,
benchmark or soak is added. PUB0T's leased delivery/copy-charge and COX0's
zero-work obligations stay with their existing owners and receiving cases.
