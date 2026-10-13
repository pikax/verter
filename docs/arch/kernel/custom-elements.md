# Custom Element producer and consumer interoperability

Custom Element declarations, registrations and consumer uses are distinct
observations. HWC3 owns their standards interpretation and CEM projection.
Framework verticals own the evidence they contribute and the behavior of their
consumers. The kernel supplies typed catalog registration, demand selection,
TypeInfo observations and snapshot-qualified candidate storage; it contains no
framework switch or second component information authority.

This decision describes the repository at `docs(arch): define atomic
contribution and candidate contracts (#879)`, 2026-10-13. The title is context,
never acceptance evidence. Under the [docs-only rule](README.md), this file is
the contract, inventory and case table. It changes no production route, test,
dependency, generated schema or gate. Implementation and executable fixtures
belong to the receiving owners below.

## Imported contracts and required outcome

The contract imports [orthogonal exact-release identities](identities.md),
[immutable catalog registration](catalog.md), [demand and activation](demand-activation.md),
[TypeInfo observations](typeinfo-facade.md), the [ComponentInfo facet view](tif1d-componentinfo-view.md),
[atomic index contributions](idx0d-contributions.md), and [receiving fact guarantees](../../../.claude/skills/component-meta/references/fact-consumer-contract.md).
Logical fields below specify those boundaries; they introduce no new public
request, identity mint, resolver or cache.

The [HTML Custom Elements standard](https://html.spec.whatwg.org/multipage/custom-elements.html)
defines registry-specific definitions, including scoped registries. The
[CEM schema](https://github.com/webcomponents/custom-elements-manifest/blob/main/schema.d.ts)
separates declarations from definition exports; a declaration can exist without
a tag. These are external interchange references, not an internal semantic
authority. HWC3 declares the exact supported CEM schema version and validates
against it; this decision adds no dependency pin.

The required outcome is static, evidence-qualified interoperability: vanilla,
same-tag different-scope, unregistered, unknown-registration and
framework-produced cases remain distinguishable. A standards association
never claims that application code ran, a module was loaded, registration
succeeded at runtime, an element upgraded, or a consumer is reachable.

Standards code may consume an admitted constructor reference, registration
operation or surface fact. It must not recognize Lit by a superclass,
interpret Vue macros or Stencil decorators, execute Svelte options, infer a
tag from a filename, or reconstruct semantics from type display strings.
Aliases and reexports require the owning symbol/package observation, not
spelling equivalence. An ordinary component is not a CE declaration just
because a template policy permits its tag.

## Required manifest dispositions

Every carrier, semantic-profile and project-profile manifest subject to
[VM13](vertical-manifest.md) declares exactly one producer disposition and
one consumer disposition. Neither missing row nor an empty array means
not-applicable. The roles are independent: producer-only, consumer-only,
both and neither are all expressible.

| Disposition      | Required contract data                                                                                                                                                                                                                                                      |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `producer`       | Exact profile/release where applicable, one vertical evidence owner, registered operation and TypeInfo facet/role references, supported declaration/registration evidence kinds, applicability and conformance references. HWC3 projects its evidence into standards facts. |
| `consumer`       | Exact profile/release where applicable, one consumer adapter owner, registered demand for standards association, binding/attribute/event/slot semantics owned by that adapter, captured policy/config dependencies, applicability and conformance references.               |
| `not-applicable` | One typed reason per role: carrier-only, project-overlay-only, or no CE production/consumption capability in the exact profile. No fabricated framework identity.                                                                                                           |

A required capability with no implementation remains required-unimplemented
under the manifest's capability law. A dynamic or unsupported input to an
implemented producer returns a typed incomplete/unavailable fact; it does not
change the manifest to not-applicable. An undemanded role performs no work.
Schema registration alone proves neither execution nor public exposure.
VIM1 implements structural validation (`VIM1-AC-R1`); HWC3 does not compile
manifests or own their activation policy.

## Declaration, registration and consumer identity

| Observation    | Subject and required distinctions                                                                                                                                                                                                                                                                                                                 |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Declaration    | Producing `SourceUnitId`, declaration/export slot and exact TypeInfo observation, with carrier profile and any semantic attachment/profile/release. Class spelling, file basename and tag are not its identity. A factory-produced class retains its producer's observation and occurrence.                                                       |
| Registry scope | Owner-qualified static registry subject, configured project and source/observation basis, with known document-global, scoped, conditional or unresolved status. The global registry is relative to its document/realm evidence, never workspace-global. Two registry expressions require an owning alias/identity proof before being equated.     |
| Registration   | Source-backed call/occurrence slot, declaration/constructor reference, registry-scope reference, exact tag value, autonomous or customized-built-in form and any `extends` local-name fact, plus independent conditions/status. One declaration can have several registrations; one registration does not become the declaration's intrinsic tag. |
| Consumer use   | Authored usage slot and source/map basis, local name, any `is` value, applicable registry/region reference, exact consuming profile and captured consumer policy. The policy admitting a CE spelling is independent of a registration or member contract.                                                                                         |
| Association    | Typed consumer, registration and declaration references; consulted candidate domain and its completeness; exact matching evidence and unresolved conditions. References retain their owning observations rather than copying component metadata.                                                                                                  |

These subjects use the imported tagged canonical encoding and collision
comparison law. Revision-local slots carry their exact basis. Source edits
retain source lineage while invalidating old observations. Package provenance
includes the resolved package instance/version, export/module resolution and
configured-project/config observation; a declared dependency range is not
exact-release evidence. Two installed copies or project contexts cannot alias
because their names match.

`CarrierProfileId` governs syntax, `FrameworkProfileId` the exact semantic
release, and `ProjectProfileId` the selected overlay. None substitutes for the
others or for configured-project membership. `CatalogSnapshot` fixes the
operation/facet schemas; `DemandPlan` selects only requested work;
`TypeInfoRequest` addresses the owning observation. No result invents one
`InputBasisId` for a composed view with multiple child bases.

The index enumerates snapshot-qualified candidates, not semantic answers.
HWC3 validates declaration, registry and registration relations through their
owners before publication. Same tag in two known scopes yields two distinct
registrations. A consumer with a proved scope can select the matching one;
an unresolved consumer scope retains typed ambiguity and all admitted
candidates. Load order, package search order and the first posting cannot
select a winner. Conflicting registrations in one scope retain conflict
evidence rather than proving which runtime definition won.

## Standards surfaces, provenance and partiality

The standards facet references independent TypeInfo facts for properties,
attributes, events, slots, methods, CSS parts and CSS custom properties. A
property/attribute mapping, reflection, conversion, default, event object or
payload role, method signature and slot name each needs its producer's own
evidence. A name or boolean inferred from a sibling is not an observation.
DOM slots are distinct from framework slots/snippets; framework callbacks are
not automatically DOM dispatches. Style surfaces link to their style owner.

Each type-bearing role retains the child `(QueryIdentity, InputBasisId)`,
declaration/occurrence slot and typed structural role/path. Follow-on demand
uses that child's TypeInfo operation. CEM `type.text` and authored annotations
remain presentation/authored syntax, never a graph recovered by parsing text.
TypeScript-owned facts retain their certified backend authority; provider
unavailability cannot trigger native replacement.

Every fact retains producer descriptor/operation, `ResultContractId`, exact
profiles/releases, source revision/content and checked authored byte origins.
Projected origins retain the producing `MapRevision` chain. Derived facts
identify all consumed observations, package/source/config dependencies,
conditions, independent quality and machine causes. Required provenance and
leases survive optional audit capture being off. A missing or expired lease
cannot be hidden by a CEM file, cached member array or enclosing summary.

| State                                     | Permitted claim                                                                                                              |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Complete closed domain                    | Positive and negative membership within the exact consulted domain; individual member values still have independent quality. |
| Complete open domain                      | Published positive members only; absence is unproved.                                                                        |
| Approximate domain                        | Lower-bound enumeration with causes; omitted declarations, registrations or members prove nothing.                           |
| Unavailable value                         | No semantic value, with machine causes; not an empty surface, `false`, TypeScript `unknown`, or a complete sibling's type.   |
| Ambiguous association                     | The known competing candidates and unresolved scope/condition; no arbitrary selected declaration.                            |
| Proved no registration in a closed domain | Declaration-without-registration for that domain, not worldwide nonexistence or runtime absence.                             |
| Unknown registration                      | Unresolved registry, constructor, tag, option or condition with its evidence; not declaration-without-registration.          |
| Undemanded facet                          | Omitted; no absence, completeness or applicability claim.                                                                    |

An association can be exact while a member type is unavailable. A member can
be present while its value or reflection boolean is unavailable. Refusal
never certifies a Complete CEM facet. Execution abort publishes no fact.
Reuse follows the receiving fact contract: certified refusal reuse retains
the original refusal, never promotes a degraded wrapper or completes a
dependent surface. No new blanket cause allowlist or Complete-only cache
policy is introduced here.

Semantic collections sort by canonical subject/role/occurrence encoding,
profile-qualified facet key and registration/scope identity. Hash-map,
interning, thread and provider registration order are irrelevant. Duplicates
with distinct occurrences survive. Signature parameters, tuples and any
semantically ordered producer sequence preserve their order;
union/intersection reduction remains `VerterStableV1`. Authored ordering and
display text are separate presentation data.

## CEM import and export

HWC3 is the sole importer/exporter of standards facts. Vertical producers
contribute evidence, never independently serialize a rival CEM authority.
CEM is a package interchange artifact: input bytes have their own source,
package/config basis and declared schema version. A malformed or unsupported
version returns a typed validation/unsupported result, never a complete-empty
manifest. An external manifest is evidence of its authored declarations; it
cannot certify current source resolution, backend types or runtime execution.

The admitted mapping is:

| Internal observation               | CEM projection                                                                                                                                   |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Package/module subject             | `schemaVersion`, `modules`, module `path`, declarations and exports; package discovery retains the captured `package.json` manifest reference.   |
| Declaration                        | Class/custom-element declaration and source/reference links, independently of registration.                                                      |
| Known document-global registration | A `custom-element-definition` export referencing the declaration; `tagName` only when the schema's self-registration rule is actually evidenced. |
| Properties and methods             | Typed member roles projected to fields/methods with parameter and return order retained.                                                         |
| Attributes/events/slots            | Their independent standards facts, field association/reflection only when evidenced, named/default slots kept distinct.                          |
| Style contract                     | CSS parts/custom properties from the style-owned facts.                                                                                          |

The [CEM declaration and export schema](https://github.com/webcomponents/custom-elements-manifest/blob/main/schema.d.ts)
does not encode registry scope or TypeInfo observation leases. A scoped
registration must not be serialized as a global definition. Unknown
registration, incomplete member enumeration and unavailable types must not
be silently omitted from an export whose result claims complete coverage.

For a schema-only export, HWC3 reports representability separately from the
quality of the input observations: every unrepresentable demanded fact has a
typed loss/refusal, while independently representable declarations remain
usable. A complete export claim covers only its explicitly admitted domain.
A scoped declaration can be exported without a global definition; that says
nothing about its scoped registration. No invented schema field makes the
file appear lossless to an ordinary CEM reader.

A lossless round-trip of admitted facts **and provenance** requires a typed,
versioned companion to the standard manifest in HWC3's result contract. Its
rows bind canonical manifest subjects/occurrences to exact owner observation,
scope, quality, conditions, source/package/config origins and any required
lease. It is transport for the same facts, not a registry or type graph.
Missing, mismatched, stale or undecodable companion evidence yields explicit
unavailability for those guarantees; CEM-only import cannot fabricate it.
HWC3 owns the concrete serialization schema, validation and tests (`HWC3-AC2`),
without an untyped extension bag or a second public request vocabulary.

Round-trip equality compares the admitted semantic facts and their preserved
provenance/status, not JSON key layout or original whitespace. Serialization
uses canonical subject/occurrence ordering and stable optional presence;
semantically ordered arrays retain producer order. Export/import performs no
runtime execution or framework-semantic discovery. Package references resolve
through PM-owned evidence; unavailable dependencies remain typed incomplete.

## Outcome and consumer inventory

All in-scope outcomes and consumers are enumerated below. Each row has one
implementation owner, a successor path from CEF0, and receiving acceptance.
The paths are predecessor-edge paths in the controller-owned plan, not a new
repository DAG. Existing imported services keep their owners. A consumer row
assigns only its CE integration, never its upstream type/compiler authority.

| Id     | Outcome                                                                                   | Sole owner; successor path; receiving acceptance        |
| ------ | ----------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| CE-O01 | Manifest-required producer/consumer dispositions and validation                           | VIM1; CEF0 → VIM1; `VIM1-AC-R1`                         |
| CE-O02 | Neutral declaration/registration/scope model, static association and typed ambiguity      | HWC3; CEF0 → HWC3; `HWC3-AC2`, `HWC3-AC-R1`             |
| CE-O03 | Standards surfaces as TypeInfo references, provenance, quality and deterministic ordering | HWC3; CEF0 → HWC3; `HWC3-AC2`                           |
| CE-O04 | CEM import/export, representability and admitted provenance round-trip                    | HWC3; CEF0 → HWC3; `HWC3-AC2`                           |
| CE-O05 | Vue CE production evidence and consuming policy/bindings                                  | VCE0; CEF0 → HWC3 → VCE0; `VCE0-AC2`, `VCE0-AC3`        |
| CE-O06 | Svelte CE production evidence and consuming policy/bindings                               | SCE0; CEF0 → HWC3 → SCE0; `SCE0-AC2`, `SCE0-AC3`        |
| CE-O07 | Lit-owned reactive properties, events, slots and methods contributed as neutral evidence  | LIT3; CEF0 → HWC3 → LIT2 → LIT3; `LIT3-AC1`, `LIT3-AC3` |
| CE-O08 | Stencil-owned props, events, slots and methods contributed as neutral evidence            | STN3; CEF0 → HWC3 → STN2 → STN3; `STN3-AC2`             |
| CE-O09 | Executable ownership inventory validation                                                 | UAO0; CEF0 → UAO0; `UAO0-AC-R1`                         |

| Id     | Consumer boundary                                                    | Sole integration owner; successor path; receiving acceptance                                      |
| ------ | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| CE-C01 | Neutral HTML/vanilla uses and package CEM import/export clients      | HWC3; CEF0 → HWC3; `HWC3-AC2`, `HWC3-AC-R1`                                                       |
| CE-C02 | Vue template/TS CE consumption and standards evidence handoff        | VCE0; CEF0 → HWC3 → VCE0; `VCE0-AC2`                                                              |
| CE-C03 | Svelte CE consumption and standards evidence handoff                 | SCE0; CEF0 → HWC3 → SCE0; `SCE0-AC2`                                                              |
| CE-C04 | Lit template CE consumption and evidence handoff                     | LIT3; CEF0 → HWC3 → LIT2 → LIT3; `LIT3-AC2`                                                       |
| CE-C05 | Stencil CE consumption and evidence handoff                          | STN3; CEF0 → HWC3 → STN2 → STN3; `STN3-AC2`                                                       |
| CE-C06 | Generated manifest/conformance consumers of both role dispositions   | VIM1; CEF0 → VIM1; `VIM1-AC-R1`, `VIM1-AC2`                                                       |
| CE-C07 | Ownership lock consuming this inventory                              | UAO0; CEF0 → UAO0; `UAO0-AC-R1`                                                                   |
| CE-C08 | Train conformance audit consuming this contract and landing evidence | TC-EXPANSION-KERNEL-1; CEF0 → TC-EXPANSION-KERNEL-1; checkpoint's frozen-charter/acceptance audit |

LIT3/STN3 own framework interpretation; HWC3 owns every standards projection
of their evidence. Vue/Svelte contributions follow the same boundary. Later
IDE, diagnostics, public adapters and cross-framework soak joins consume these
facts through their own charters; they are not extra standards/type authorities
or executable acceptance added to this docs-only node.

## Displaced routes and sole cutover owners

The [authority inventory](authority-inventory.md) already assigns existing
production routes. This contract references those deletion units unchanged:
it does not transfer their implementation into CEF0 or create duplicate
assignments. Current paths/symbols remain the evidence; an upstream owner
reconciles moves before its cutover.

| Category                                  | Existing displaced production routes used by CE callers                                                                                                          | Sole existing deletion owner; existing path; receiving acceptance |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| Central framework switch                  | UAK0 `D01`–`D07`: `CarrierGrammarConfig`, carrier/request capability matches, parse identity and hard-coded framework registrations in language/compiler/session | CPF1; UAK0 → UAK1 → CAT0 → CPF1; `CPF1-AC1`                       |
| Central framework switch                  | UAK0 `D10`, `D11`: framework branches in LSP/editor and MCP                                                                                                      | COX0; UAK0 → UAK1 → CAT0 → DEM0 → COX0; `COX0-AC1`                |
| Untagged public identity                  | UAK0 `D08`: `FileLanguage` adapter/language conflation                                                                                                           | CPF1; UAK0 → UAK1 → CAT0 → CPF1; `CPF1-AC1`                       |
| Untagged public identity                  | UAK0 `D09`: capability/global framework gates                                                                                                                    | COX0; UAK0 → UAK1 → CAT0 → DEM0 → COX0; `COX0-AC1`                |
| Untagged public identity                  | UAK0 `D19`: project registry authority                                                                                                                           | PM1; UAK0 → UAK1 → VID0 → CFG0 → PM0 → PM1; `PM1-AC1`             |
| Untagged coordinate                       | UAK0 `D16`: native/WASM/FFI/MCP/proto positions                                                                                                                  | ENCF0; UAK0 → UAK1 → ENC0 → ENCF0; `ENCF0-AC1`                    |
| Untagged coordinate                       | UAK0 `D17`: fixed UTF-16 editor contracts                                                                                                                        | ENCL0; UAK0 → UAK1 → ENC0 → ENCL0; `ENCL0-AC1`                    |
| Duplicate component information authority | UAK0 `D12`–`D15`: component-meta resolver/schema/cache, public-API projection, off-store surface caches, legacy TypeInfo DTOs                                    | TIF1; UAK0 → UAK1 → CAT0 → TIF1; `TIF1-AC1`                       |
| Duplicate component information authority | UAK0 `D18`: per-request component scan                                                                                                                           | IDX0; UAK0 → UAK1 → CAT0 → DEM0 → IDX0; `IDX0-AC1`                |

The referenced units have their complete paths, symbols and successor paths
in `tests/kernel/UAK0/products/deletion-retag-ledger.v1.json`. Their owners
are upstream implementation dependencies, not invented CEF0 descendants.
CE-specific structural rejection/cutover obligations have these descendant
owners:

| Id     | Route or assumption rejected                                                                                                                                    | Sole later production-capable owner; successor path; receiving acceptance |
| ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| CE-D01 | Central framework switch in neutral standards projection: interpreting framework macros/decorators/superclasses or recovering a tag from `.ce.vue`/display text | HWC3; CEF0 → HWC3; `HWC3-AC1`, `HWC3-AC2`                                 |
| CE-D02 | Untagged CE association: tag-only/global-registry certainty, package/path-only subjects or unqualified coordinates                                              | HWC3; CEF0 → HWC3; `HWC3-AC1`, `HWC3-AC-R1`                               |
| CE-D03 | Duplicate component information authority in standards/CEM: independent evaluated member graph, CEM-owned types or metadata fallback bypassing TypeInfo         | HWC3; CEF0 → HWC3; `HWC3-AC1`, `HWC3-AC2`                                 |
| CE-D04 | Legacy shared WCP schema/registry and serialized/generated consumers, if discovered before final cutover                                                        | CEC0; CEF0 → HWC3 → VCE0 → CEC0; `CEC0-AC1`                               |

**Empty current population:** no `CustomElementDeclaration`,
`CustomElementRegistration`, `RegistryScope`, `CemModule` or shared WCP
schema/registry implementation exists in the named language/session/protocol/
identity roots. CE-D01–03 are structural rejection of forbidden successor
routes, not claims that those implementations already exist. CE-D04 has zero
current deletion units, consistent with UAK0's empty population. CEC0 alone
owns eventual shared-schema deletion after Vue/Svelte migration; HWC3 migrates
only neutral standards rows/adapters and must not delete that shared authority.

Current CE-related input/output is not that empty population:
`CarrierGrammarConfig` and `ParseOptions.custom_elements` encode Vue parsing
policy; session compile profiles distinguish CE build mode from template
policy; protocol `FfiSvelteCustomElementDescriptor` carries Svelte compiler
tag/props/shadow/extend evidence. VCE0/SCE0 consume and qualify these in their
own CE adapters. They must not mistake a compile option for registration or
runtime reachability. Existing carrier/compiler functionality is retained by
its owning migration, rather than deleted as a fictitious WCP registry.

## Discriminating association and interchange cases

These are required fixture contracts, not newly added tests or claims of
implemented CE behavior. HWC3 receives all standards cases under
`HWC3-AC-R1` and `HWC3-AC2` in its existing lanes. Vertical cases supply
owner-produced evidence; HWC3 must not synthesize it by understanding those
frameworks. A neutral typed evidence fixture may exercise projection before
the vertical producer lands, but cannot certify that producer's capability.

| Id     | Input/control                                                                                                                    | Required distinction; forbidden result                                                                                           | Executable acceptance owner                                                                     |
| ------ | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| CE-T01 | Vanilla exported class, explicit static constructor/tag/registry and HTML use in that scope                                      | Exact static association with sourced members; no execution/reachability claim                                                   | HWC3; `HWC3-AC-R1`                                                                              |
| CE-T02 | `x-panel` defined by different constructors in scopes A/B; consumers with proved A/B and unknown scope                           | Two distinct identities; scoped consumers select only their scope; unknown scope is ambiguous, never first-match                 | HWC3; `HWC3-AC-R1`                                                                              |
| CE-T03 | Exported CE declaration with no registration in a proved closed candidate domain                                                 | Declaration preserved, no definition export; domain-qualified absence, never an invented tag/global absence                      | HWC3; `HWC3-AC-R1`                                                                              |
| CE-T04 | Dynamic tag, registry, constructor or registration options; partial candidate domain                                             | Unknown registration with causes differs from T03; no complete-empty association                                                 | HWC3; `HWC3-AC-R1`                                                                              |
| CE-T05 | Vue ordinary/CE variants, admitted aliases/reexports and userland same-spelling factory; consuming `isCustomElement` policy edit | Producer mode and policy stay distinct; captured policy invalidates; filename/spelling alone proves neither CE nor registration  | VCE0 produces evidence (`VCE0-AC2`, `VCE0-AC3`); HWC3 checks projection (`HWC3-AC-R1`)          |
| CE-T06 | Svelte ordinary/CE variants, static/dynamic options, explicit registration and scoped consumer                                   | Exact release/mode-specific observation; dynamic options incomplete; no variant collision                                        | SCE0 produces evidence (`SCE0-AC2`, `SCE0-AC3`); HWC3 checks projection (`HWC3-AC-R1`)          |
| CE-T07 | Lit/Stencil evidence plus equivalent neutral member facts, including differing event/slot semantics                              | Framework provenance survives; neutral projection matches only admitted facts, never superclass/decorator heuristics             | LIT3/STN3 produce their evidence (`LIT3-AC3`/`STN3-AC2`); HWC3 checks projection (`HWC3-AC-R1`) |
| CE-T08 | Identical tag/export names in two package instances/projects; aliases resolving to one declaration                               | Package/project identities remain distinct; proven aliases share their owner reference; names do not merge subjects              | HWC3; `HWC3-AC2`                                                                                |
| CE-T09 | Closed empty/open/approximate member domains; present member with unavailable type or reflection; audit capture off              | Independent presence/value/domain quality and required provenance survive; refusal cannot complete CEM                           | HWC3; `HWC3-AC2`                                                                                |
| CE-T10 | Declaration/global definition CEM round-trip; scoped and unknown registrations; missing/stale companion; unknown schema          | Preserve admitted facts/provenance and ordering; scoped-to-global export refused; missing evidence unavailable, never fabricated | HWC3; `HWC3-AC2`                                                                                |
| CE-T11 | Customized built-in local name/`is`/`extends` and autonomous control; conflicting registrations in one scope                     | Distinct registration forms and conflicts; no tag-only matching or runtime winner                                                | HWC3; `HWC3-AC-R1`                                                                              |
| CE-T12 | Producer-only, consumer-only, both, neither; missing role row and unsupported exact profile                                      | Two explicit dispositions; missing row fails structurally; unsupported demand is not not-applicable                              | VIM1; `VIM1-AC-R1`                                                                              |
| CE-T13 | Incremental registration/scope/package/config/map edit versus fresh demand; stale/cancelled and selected-undemanded controls     | Current answers equal fresh; no torn publication or degraded wrapper admission; undemanded projection is zero                    | HWC3; `HWC3-AC3`, `HWC3-AC4`                                                                    |

## Evidence selection and acceptance boundary

CEF0-AC1 is the reviewed inventory and explicit empty-population evidence
above. UAO0's `UAO0-AC-R1` receives executable validation of missing members,
unknown/pathless owners and conflicting assignments. It reconciles this
Markdown inventory into its receiving data format; CEF0 adds no validator,
test registration, duplicate node table or repository-side DAG.

CEF0-AC2 cites existing coverage for the imported boundaries, with deliberately
narrow claims:

| Existing evidence                                                                                                                                                         | What it discriminates; limit                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/verter_language/tests/cases/registered_authorities.rs`: `public_authority_api_round_trips_only_sealed_values`                                                     | Accepted source retains its sealed snapshot identity, bytes and registered grammar; not a CE registration proof.                       |
| `crates/verter_language/tests/cases/parse_identity.rs`: `vue_custom_element_order_and_duplicates_are_irrelevant`, `vue_parse_affecting_options_change_the_syntax_profile` | Canonical policy ordering and relevant policy identity changes; not semantic registry scope or exact framework-release implementation. |
| `crates/verter_protocol/tests/cases/typeinfo_proto_roundtrip.rs`: `framework_surface_origin_hop_presence_roundtrips`                                                      | Origin absence stays distinct from present id zero, preserving source-chain presence.                                                  |
| Same file: `supported_empty_and_unsupported_empty_decode_to_distinct_states`                                                                                              | Unsupported surface cannot alias supported-empty; not a proof of every independent fact-quality lane.                                  |
| Same file: `semantic_type_graph_roundtrip_preserves_envelope_shape`, `framework_surface_payload_roundtrips`                                                               | Imported query/graph and ordered surface payload transport; not the future CEM serializer or successor profile newtypes.               |
| `crates/verter_session/src/runtime_render_lane_tests.rs`: `runtime_render_honors_vue_custom_element_script_profile`                                                       | Existing CE script compile mode is distinct from ordinary mode; it does not establish a standards registration.                        |

The imported identity/facet/index contracts remain normative; existing tests
do not falsely certify their unimplemented successor schemas. HWC3 owns new
or extended standards/CEM tests, including CE-T01–11 and CE-T13; VIM1 and the
vertical owners keep their receiving obligations. No new test is necessary
to discriminate this documentation change.

CEF0-AC3 is not applicable to execution in this change: no index delta,
cache key, cancellation, publication or reuse authority changes. The imported
IDX0D and TypeInfo/fact laws govern those authorities; HWC3-AC3 and vertical
incremental cases prove their implementation, including CE-T13.

CEF0-AC4 is not applicable to execution in this change: no parse, resolve,
plan, emit, copy, allocation or retained candidate path changes. Demand-driven
shared parsing and TypeInfo reference reuse stay required. HWC3-AC4 proves
bounded projection work in implementation; this node adds no counter, timer,
benchmark or soak.
