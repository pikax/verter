# ComponentInfo as a TypeInfo view

ComponentInfo is a versioned view of TypeInfo observations with open, tagged
framework facets. It has no resolver, type graph, semantic cache or publication
authority of its own. The current component-meta and public-API orchestration
converge on this view in TIF1; the shared type engine remains the sole native
type authority. Compatibility output is a mechanical presentation projection.

This decision describes the repository at `fix(supervise): verify parent
identities before attributing exit CPU (#872)`, 2026-10-12. That title is
descriptive context, not acceptance evidence. It follows the [docs-only
rule](README.md): this file contains the contract, inventory and case table;
it changes no production route, transport, dependency, test or gate.

## Imported contracts and intent

The view uses [TypeInfo's selectors, operations and observation
law](typeinfo-facade.md), [typed catalog registration](catalog.md),
[profile and exact-release identities](identities.md), and the current
[native fact transport](../../../.claude/skills/component-meta/references/native-facts.md).
It preserves the [receiving fact
guarantees](../../../.claude/skills/component-meta/references/fact-consumer-contract.md).
DX0's `FeatureExposureContract`, `HostExecutionClass` and
`ProductReceiptBasis`, recorded in `tests/product-experience/DX0/products/`,
govern exposure; this view does not introduce another capability catalog.

The required outcome is one component observation that preserves current
Vue/Svelte semantics, source identity, per-fact quality and consumer behavior.
Frameworks contribute their own facets rather than being coerced into required
Vue props/events/slots. A consumer must declare semantic facts, presentation
compatibility, or both as separate demands. No fallback reads display text to
reconstruct meaning. No facet evaluates a TypeScript-owned fact natively.

TIF1 owns the universal view, adapters and atomic retirement (`TIF1-AC1`–`AC3`,
`TIF1-AC-R1`). Framework projection nodes own additional framework semantics;
for example PRE3 owns Preact callback-prop/children derivation and unsupported
Model/Options (`PRE3-AC3`), and refused TypeScript operations (`PRE3-AC4`).
The schema itself is this decision's `TIF1D-AC2`; UAO0 receives the ownership
table validation (`UAO0-AC-R1`). Neither docs-only owner is a production
deletion owner.

## View, identity and type roles

The following is a logical schema, not a new Rust/TS DTO or a second request
vocabulary. Its fields fit `OP10 ComponentFacets` and the existing TypeInfo
result envelope (`TR06`, `TR14`, `TR18`–`TR26`).

| View element | Contract |
| ------------ | -------- |
| Version | ComponentInfo's wire/view schema is a serialization version. An incompatible semantic result contract changes the descriptor's `ResultContractId`; a wire layout change alone does not. |
| Subject | The typed `Component` selector: `SourceUnitId`, exact export name and `FrameworkProfileId`, under the selected configured project and selector basis. File basename is display only. |
| Request | `OP10`, requested facet keys and their semantic demand: path, mode, reduction and closure. The host captures a current basis when requested; unknown selectors, profiles or malformed arguments fail validation before execution. |
| View identity | The existing OP10 `QueryIdentity`, composed from the selector, canonical requested facet set/arguments, observed profiles and `ResultContractId`. No ComponentInfo-specific key, basis, flight or coalescer is minted. |
| Root | The full TypeInfo fact result when OP10 is Native, or the ordered TypeInfo child results when OP10 is Composed. A composition container has no `InputBasisId`. |
| Facet | Profile-qualified kind key, payload schema version, applicability, independently guaranteed facts, provenance and optional presentation data. An omitted, undemanded facet claims nothing. |
| Type role reference | The owning child fact's `(QueryIdentity, InputBasisId)` and node decl-slot identity, plus the structural role within that fact. This is the `NodeRef` address of `TR06`, not an opaque live handle or a new observation identity. |
| Summary | Derived only from the facts actually published for this demand. It never overrides an independent member/domain guarantee or proves a missing field is absent. |

Every type-bearing field refers to its exact TypeInfo observation. A role
identifies, for example, input value, event signature/parameter/return, slot
input/binding/return, model value, options member, exposed member or
accepted/inherited member.
The role path uses typed structural segments and positions within the owning
signature or tuple; it is not a string parsed from a type display. Member
occurrences include their producer's declaration/occurrence slot, so duplicate
event names and overloaded signatures are never collapsed by name alone.

A reference resolves only in that fact's basis and observed profile. It cannot
name the composed container, reinterpret a node id from another graph, or
collapse child bases into one `InputBasisId`. Follow-on expansion uses
`OP02 ProjectPath`, `OP04 ExpandAround` or `OP09 ShallowSurface` on the owning
child's `NodeRef`. Shared memo authority stays in `ProjectTypeStore` and
`verter_type_engine`; the view retains references, not copies of evaluated IR.
The same child/role may serve several facets without another resolution.

Native macro/declaration facets are answered by `ProjectSemanticDispatch`.
If any requested facet needs a TypeScript-authoritative fact, OP10 takes its
Composed claim: TIF1 assembles the children and executes none. The relevant
child uses `EngineBackend` over `CertifiedTypeEngineBinding`. No binding is
`Unavailable(NoProject | ProviderUnavailable | NotReady)`, never a native
replacement. Backend handles, leases, pointers and sessions do not cross the
DTO. Consumer declarations are not authority-selection heuristics.

## Tagged facet registration and provenance

A kind is a typed, namespaced row referenced from the catalog's `T03` semantic
profile surface kinds and `T07` capability cells. It names one payload schema,
semantic demand/role map, producer owner and exact-release applicability.
Rows are immutable after snapshot construction; duplicate kind ownership or
missing references fail construction. There is no new mutable registry or
shared enum with an arm for each framework. Additional families contribute
owner-local rows without adding a neutral dispatch switch.

The capability/exposure row binds the operation, profile and exact release to
its producer owner, maturity, RequiredCurrent obligation, host execution class,
semantic/inspection rail, executable route and replay case. A registered schema
is not an executable exposure. A NativeOnly operation cannot be advertised as
browser-local without its owning browser build/route evidence; a playground
gap keeps product promotion blocked without weakening existing consumers.

Kinds such as inputs, outputs, children/yields, models, exposed API, options,
accepted surface and root inheritance are conceptual categories, not universal
required fields. A framework's row declares which category and semantics it
implements. Multiple claims on one carrier are profile-qualified and use the
catalog's coexistence/activation law; the view never picks the first profile.

Each facet and independently published fact retains:

- The producer authority, descriptor, query identity, `ResultContractId`,
  observed profiles and the owning fact's `InputBasisId`.
- The contributing `FrameworkProfileId` and exact `ReleaseId`; no display
  family or adapter string substitutes for them. Derived facts link to the
  observations they consumed and state their derivation (for example callback
  prop → output, or children → child surface).
- Source origins as `SourceUnitId` and `SourceByteRange` in `CD1`, with source
  revision/content and, for projected origins, the owning `MapRevision`.
  Imported/inherited origins name the producing source, not the requesting
  parent. Multiple origins remain multiple. An unavailable origin is explicit,
  with cause; never a fabricated zero span or parent-file offset.
- Independent quality and causes, availability predicate/branch references,
  publication diagnostics and reuse class. Engine/backend identity remains
  value-side provenance; optional traces/origin graphs are capture only.

Canonical facet order is the canonical encoding of the profile-qualified kind
key. Fact/member order uses canonical subject/role/occurrence encoding;
union/intersection ordering remains `VerterStableV1`. Signature parameter,
tuple and structural path positions keep their semantic order. No load,
interning, hash-map or thread order changes equality. Source occurrence order,
overload presentation, root-target display order, JSDoc and formatting ride a
separate presentation projection; reordering that projection cannot change a
semantic key or discard a duplicate occurrence. A framework rule whose
authored ordering changes meaning must record that ordering in the semantic
observation, rather than treating it as cosmetic.

## Partiality and applicability

Quality uses the current `FfiFact`/`FactResult` law. Availability and membership
are separate axes, as are domain quality, member value, boolean facts and
authored syntax. The schema does not replace the transported guarantees.

| Answer | Value and claim |
| ------ | --------------- |
| `Complete` | Value present, no degradation cause. `false`, empty collections and complete optional absence are valid values, not refusals. |
| `Approximate` | Value present with nonempty machine causes. A member domain is a lower bound; omitted members prove nothing. |
| `Unavailable` | Nonempty machine causes and no value. Unsupported, missing dependency, missing project, provider limitation and failed publication do not become empty success. |
| Closed member domain | Complete coverage permits positive and negative membership claims. Each member's type/booleans still has its own guarantee. |
| Open member domain | Even Complete proves only published positive members. Approximate enumeration stays open. |
| Conditional availability | A member/facet names a typed predicate or stable branch references and the observations needed to decide it. An exact conditional description can be Complete; uncertainty evaluating it has its own Approximate/Unavailable status. |
| Unsupported applicability | A requested registered facet that the exact profile/release does not support returns `Unavailable` with an unsupported cause and profile/kind provenance. It has no semantic value. |
| Undemanded facet | Omitted from the requested results, neither unsupported nor complete-empty. |

Unsupported applicability is carried by the unavailable arm's open numeric
cause vocabulary, with structured applicability in facet provenance. New
cause allocations or payload kinds belong to the implementing transport
change; this decision assigns none. An older reader retains unknown codes
and does not infer availability from a recognized-code allowlist. An unknown
payload version/kind cannot be interpreted as a supported empty facet; an
adapter reports the unsupported decoding capability while retaining the
producer's status/provenance where decodable.

A failed member publication has no type value but does not erase independently
proved membership or healthy siblings. A boolean fact that was not established
does not become `false`. Refusal and recovery keep their original causes; a
legacy summary or compatibility array cannot certify a closed domain. Execution
abort publishes no fact. Legacy transport adaptation remains conservative.

For Preact, callback props and children/render props contribute derived output
and child facets with their input observations; Model and Options are explicitly
unsupported. For Svelte, runes props, callbacks and snippets retain their own
semantics; a legacy slot binding with unavailable/opaque precision stays
truthful and cannot borrow precision from a named-symbol callable observation.
No facet is made universally mandatory by these examples.

CSS parts and custom properties are links to style/DOM facts, with independent
profile, source basis and status. Accessibility and runtime observations link
in the same way. None changes a prop's type, completeness or semantic cache
identity. Runtime evidence additionally names its execution basis; absence of
an observation is not a negative type fact.

## Field inventory

Inventory sources are `packages/component-meta/src/types.ts` and
`native-component-meta.ts`, `crates/verter_session_query/src/analysis/component_meta.rs`,
`crates/verter_protocol/src/types.rs` and `component_facts.rs`, the full
`component_meta.proto` and `selective_component_meta.proto`, and session
`component_meta_payload.rs`. CamelCase names below include their snake_case
Rust/protobuf mirrors. A grouped row enumerates its fields and gives all of
them one cutover owner and receiving acceptance. It does not give the view
ownership of the upstream parser, CSS analyzer or type solver.

| Id | Current fields, including nested fields | View destination / distinction | Later owner; receiving acceptance |
| -- | -------------------------------------- | ------------------------------ | --------------------------------- |
| F01 | `filePath`, derived `componentName`, selector export and adapter/profile | Typed component subject; file basename/path spelling is presentation | TIF1; `TIF1-AC2` |
| F02 | `optionsApi` | Profile-specific options facet; not a universal boolean | TIF1; `TIF1-AC2` |
| F03 | `props`: `name`, `type`, `required`, `hasDefault`, `default`, `runtimeTypes`, `callable_role`, `declaredInMacroTypeArg`, `description`, `tags` | Input membership, value role and independent boolean/default facts; authored identity retained | TIF1; `TIF1-AC2` |
| F04 | `events`: `name`, `payload`, return source/publication, `hasValidator`, `isDeclared`, `rawSignature`, `description`, `tags` | Output occurrence/signature roles, payload parameters and return; raw signature is display only | TIF1; `TIF1-AC2` |
| F05 | `slots`: `name`, `isScoped`, `bindings`, `isRequired`, `hasFallbackContent`, `returnType`, return value/publication/scope, `declaredInMacroTypeArg`, `description`, `tags`; bindings: `name`, `type`, `expression`, `rawType` | Child/yield membership with binding and return roles; display return string never proves a type | TIF1; `TIF1-AC2` |
| F06 | `models`: `name`, `type` | Model value role; profile rules may link it to input/output facts | TIF1; `TIF1-AC2` |
| F07 | `exposed`: `name`, `type`, `description`, `tags`; `publicInstance`: `completeness`, `members`; member `name`, `kind`, `type`, `rawType`, `description`, `tags` | Distinct exposed and runtime public-instance facets; compat exposed may project the latter, never redefine native exposed | TIF1; `TIF1-AC2` |
| F08 | `typeRegistry`: `name`, `type`, `rawType`, `declaration`; declaration `requestedName`, `resolvedName`, `canonicalSource`, `spanStart`, `spanEnd`, `kind`, optional `text` | Demanded TypeInfo child references and declaration provenance; no metadata-owned registry graph | TIF1; `TIF1-AC2` |
| F09 | `acceptedProps`: `name`, `type`, `rawType`, `required`, `kind`, `callable_role`, publication, effective `type_source_scope`; `acceptedEvents`: `name`, `payload`, `rawSignature`, effective `payload_scope`; both `provenance`, `availability` | Accepted input/output roles reference declared or inherited producing observations; retain branch conditions and declaring scope | TIF1; `TIF1-AC2` |
| F10 | `acceptedSurfaceCompleteness`; member provenance `declared` / `inherited.sources`; sources `nativeTag.tag` / `component.canonicalId`; availability `always` / `conditional.branchKeys` | Derived domain summary plus typed provenance/conditional availability, never member type authority | TIF1; `TIF1-AC2` |
| F11 | `rootReachability` discriminants/reasons/branches; branch `branchIndex`, `conditionText`, `target`, `consumed`, `hasUnknownSpread`; `rootInfo.kind`, `reason`, `targets` | Root facet plus derived presentation summary; condition text is diagnostic only | TIF1; `TIF1-AC2` |
| F12 | Root target `kind`, `elementIndex`, `usageIndex`, `tag`, `name`, `importSource`, unresolved `reason`; consumed `attrs`, `listeners`, `hasDynamicAttrName`, `hasDynamicListenerName` | Links to registered carrier/usage observations, typed causes and exact branch identity; ordinal display cannot establish a cross-revision target | TIF1; `TIF1-AC2` |
| F13 | `fallthroughSurface` kind/reason/branches; branch `branchKey`, `conditionText`, `props`, `events`, `rootChain`, `status`; prop `name`, `type`, `rawType`, `sources`, callable role/publication/effective scope; event `name`, `payload`, `rawSignature`, `sources`, effective scope; chain `kind`, `tag`, `canonicalId`, `componentName`, `reason`; partial/unresolved reasons and generic failure | Branch-structured inheritance; member roles reference producing child observations, including multi-hop effective scope | TIF1; `TIF1-AC2` |
| F14 | `componentPublicContract`: supported/unsupported, adapter id, exactness, degradation, provenance, output lane/index/inner index, publication/output failures, diagnostics; public props optional/default/type; events source overloads, handler shapes, parameter names/type/optional/rest and return; slots optional/input bindings/type/return | A projection of the same fact/role references, not `ComponentContractEnvelope` or another type graph; unsupported/error context preserved | TIF1; `TIF1-AC2` |
| F15 | Every type lane's `publication`, `terminalDisplay`, `typeExpansion` / `payloadExpansion`: kind, failure/absence, semantic authority, provenance, exactness, reason, policy reason, symbolic proof, causes; expansion execution status and diagnostic reason/context/property; `rawType`, `typeAnnotation`, `rawSignature` | Guarantee/diagnostic provenance on the owning observation; terminal/raw text remains presentation or authored syntax, never resolved type truth | TIF1; `TIF1-AC2` |
| F16 | `facts`: props/events/slots/models/exposed/acceptedProps/acceptedEvents/rootReachability/fallthroughSurface/publicInstance/componentPublicContract/typeRegistry, plus `delivery`; `domain_guarantees` kind/causes/open; `required_fact`, `has_default_fact`; `resultCompleteness` kind/reasons | Current per-domain/member/boolean guarantees retained independently; summaries derived | TIF1; `TIF1-AC2` |
| F17 | `resolution`, `resolutionStatus`: mode, resolved/unavailable cause, macros (`macroIndex`, `macroKind`, `typeName`, `importSource`, declaration, nativeProps, jsdoc); native prop `name`, optionality, annotation, visibility, spans; resolved JSDoc description/tags and tag params | Observation/provenance links and authored documentation; no separate resolution authority, guessed spans or text recovery | TIF1; `TIF1-AC2` |
| F18 | `macroExpansionDiagnostics`: macroKind/index, exactness, executionStatus, diagnostics; `origin` nodes id/kind/label, edges source/target/kind/meta, meta strings; request/audit id and compute counters | Required diagnostic status separate from optional captured trace/audit presentation; capture off preserves correctness | TIF1; `TIF1-AC2` |
| F19 | `orderedSfcStructure`: schemaVersion, artifactToken, blocks, markupNodes; nested section/markup oneofs, sealed block/node/attribute/source-space/content-basis tokens, authored names, ranges, termination, attributes and insertion anchors | Links to registered carrier inventory, not a copied parse or ordinal fallback; preserve the existing nested schema | TIF1; `TIF1-AC2` |
| F20 | `components`: name/importSource/isDynamic/props/hasSpread/slotsUsed/staticClasses/hasDynamicClass/vModels/vModelEntries/bindings/events; prop name/isBound/constness/expression/referencedBindings/fromSpread/isShorthand; model bindingName; binding name/modifiers; event name/handlerExpression/isInline/modifiers | Usage facet links to existing authored analysis; callback-event classification requires the child's semantic facet, not an `on*` name guess | TIF1; `TIF1-AC2` |
| F21 | `templateRefs`: name/isDynamic/targetTag; `imports`: source/isTypeOnly/bindings (name/kind/importedName/isTypeOnly) | Authored usage/import links; resolved ref target/type, if demanded, has its own TypeInfo role | TIF1; `TIF1-AC2` |
| F22 | `bindings`: name/kind/reactivityKind/returnWrapperRole/returnWrapperUnresolvedReason/typeAnnotation/usedInTemplate/usedInStyle; `vueApiCalls`: api/argValue | Profile-specific script facts; unresolved wrapper role stays distinct from proved none; annotation is authored data | TIF1; `TIF1-AC2` |
| F23 | `styles`: lang/blockToken/scoped/isModule/moduleName/classes/ids/customProperties/vBinds/selectors; selector text/specificity | Style facet links to analyzer and sealed block observations; CSS custom-property names do not alter type facts | TIF1; `TIF1-AC2` |
| F24 | `flags`: asyncSetup/hasReactiveState/hasComputed/hasWatchers/hasLifecycleHooks/hasProvide/hasInject/hasInheritAttrsFalse/hasStoreUsage; internal `has_macro_failure` | Profile-specific facts or derived summaries; failure flag never overrides per-fact guarantee | TIF1; `TIF1-AC2` |
| F25 | Full payload schemaVersion/typeGraph strings/nodes/body/originGraph; selective surfaceSchemaVersion/eager bytes/lazy arrays; `NamedTypeHandle` name/handle/required/doc; TypeHandle project/canonical/content hash/query path (declaration fingerprint, subexpression parent/child kind/index, instantiation base/type-args fingerprint); TypeExpansion handle/shape/children, shape property/parameter/arm/element counts, return presence, literal value or primitive kind; separate bridge depth/frontier/file, stale-handle/project-mismatch/eviction and batch errors | Wire/presentation projection of exact child references; graph string/node ids are serialization-local, not identity; typed errors retained | TIF1; `TIF1-AC2` |
| F26 | Additional exact-release framework facet kinds, including Preact callback outputs/children/expose and unsupported Model/Options | Owner-local contributions using this schema, not additional Vue fields | PRE3 (Preact population); `PRE3-AC2`, `PRE3-AC3`, `PRE3-AC4` |
| F27 | Existing framework-surface `kind`, resolved/partial/unsupported/missing outcome, guarantee, diagnostics, props/emits index signatures, defaults and origins; options/expose members name/optional/value/annotation/references/span; emit occurrence id/span/payload/source/scope/publication/return; model bindings name/prop; slot return/member rows; prop authority/authored evidence/callable role | OP10 facets with exact child/role references, including options values and index key/value roles; preserve origin-hop chain (local/import/reexport/alias) and status distinctions; display annotations do not become meaning | TIF1; `TIF1-AC2` |

`F26` assigns only the named Preact population. Each other future framework
contribution is assigned by its own projection charter (the framework's
`<P>3` node where applicable) and must name its own receiving acceptance when
its rows are added. This decision does not assign an empty future population
to an invented owner. Existing Vue/Svelte populations `F01`–`F25` and `F27` migrate
under TIF1. A nested type graph node has the owning TypeInfo graph's semantics
and recursion identity; its fields are not a second component schema.

## Consumer inventory

Each row owns adoption of the view at the listed boundary, not reimplementation
of the consumer's product or upstream producer. Consumers needing both rails
use separate semantic and presentation demands; there is no unclassified
current consumer.

| Id | Current consumer / source | Required rail and preservation | Adoption owner; receiving acceptance |
| -- | ------------------------- | ------------------------------ | ------------------------------------ |
| C01 | Session `component_meta_host.rs`, `meta.rs`/`meta/output_api.rs` (`MetaSession`), host `get_component_meta*`, batch and encoded-payload delivery | Semantic facts; captured/overlay view, batch order, per-item errors and one fixed basis capture | TIF1; `TIF1-AC2`, `TIF1-AC3` |
| C02 | Session `framework/api_projector.rs`, `framework/public_contract.rs`, `get_public_api*` and declaration/public/testing projections | Semantic roles → declaration presentation; preserve structured public contract and authored mappings; emitted code is not a type oracle | TIF1; `TIF1-AC2` |
| C03 | `verter_ffi::convert::component_meta_output_to_ffi`, protocol JSON/protobuf conversion, `packages/proto/src/component-meta.ts` | Mechanical transport; retain status, causes, localized failure and schema compatibility | TIF1; `TIF1-AC2` |
| C04 | NAPI `meta.rs`/host methods, `packages/native`, component-meta runtime project-engine/project-session | Semantic request/transport; scalar, resolved alias, audited, batch, overlay, selective and expansion entries share the view | TIF1; `TIF1-AC2` |
| C05 | WASM `WasmMetaProject`/`WasmMetaSession` and host framework-surface methods, `packages/wasm` | Same semantic/transport contract with browser-host capability limits explicit | TIF1; `TIF1-AC2` |
| C06 | `packages/component-meta` index/browser exports, native decoding, type-graph/proto-decode, type-ir descriptors, `nativeComponentMetaToComponentMeta` | Semantic decode plus separate descriptor presentation; recursive/shared references remain mechanical, no JS solving | TIF1; `TIF1-AC2` |
| C07 | `ComponentMetaChecker`, `mapComponentMeta`, compat native-projection/types and `ComponentMetaSession` project API | Presentation compatibility over one completed native response; declared/accepted policies, runtime constructor display, schemas, defaults and public-instance exposure remain projections | TIF1; `TIF1-AC2`, `TIF1-AC-R1` |
| C08 | Component-meta JSON Schema and Zod adapters | Semantic facts for representable validation; unsupported/shallow facts must not manufacture exact validators | TIF1; `TIF1-AC2` |
| C09 | Component-meta Storybook/Histoire adapters (argTypes, controls, default props, variants) | Presentation suggestions from semantic input/default facts; generated examples do not establish type truth | TIF1; `TIF1-AC2` |
| C10 | LSP `server/custom_methods/component_meta.rs`, `protocol_types.rs`, `main.rs`; language-shared request types and VS Code test command forwarding | Semantic full/selective/expansion transport; malformed URI is an error, genuine non-component is absence, failed materialization/abort is not null | TIF1; `TIF1-AC2` |
| C11 | LSP `documents/analysis.rs`, `server/component_resolve.rs`, `server_utils.rs`, diagnostic/hover/slot consumers | Semantic member/required/default/slot/root and macro diagnostic facts; native diagnostic inspection stays distinct from TypeScript authority | TIF1; `TIF1-AC2` |
| C12 | LSP provider synchronization, background declaration closure, workspace scan, carrier sync and rename/sync mapping readers of `get_public_api*` | Declaration presentation and authored mappings from the semantic view; provider readiness and edit safety remain with their current owners | TIF1; `TIF1-AC2`, `TIF1-AC3` |
| C13 | LSP `server/nav_features_hover_provenance.rs`, NAPI/WASM audited bundles, `packages/native/audit.ts` and `packages/wasm/src/audit.ts` | Optional inspection presentation plus required observation provenance; audit queries do not recompute meaning | TIF1; `TIF1-AC2` |
| C14 | MCP framework-surface tool (`tools/framework_surface.rs`, server/tools routing) and `packages/typeinfo/src/framework-surface.ts` | Semantic kinds/member roles with status/origin, then JSON presentation; missing support is explicit | TIF1; `TIF1-AC2` |
| C15 | Benchmark component-meta runners, corpus traces, audit-validator and dx-harness churn scenario | Semantic/compat answer comparisons and optional work inspection; preserve separate rails, never gate type correctness on display/timing | TIF1; `TIF1-AC2`, `TIF1-AC4` |
| C16 | Playground `componentMeta` output-mode alias and unconnected audit Why tab | Existing analysis presentation only; these are not live component-meta query consumers. Preserve the alias; no new producer/UI is promised here | TIF1; `TIF1-AC2` |

IDX0/IDX0D receive this contract for index identity and contributions: entries
name the view's existing query identity and observations, and serve candidates,
not types (`TIF0 TR10`). CEF0, PUB0, CWB0, LRA0 and NCK0 consume the view or
its status/provenance law for their own contracts. These are future contract
consumers, not extra current call sites or second owners of `C01`–`C16`.
Workbench interactivity, capability promotion, diagnostic algorithms and
accessibility/runtime/style producers remain their own chartered outcomes.

## Retirement inventory

No deletion occurs in this decision. The following are TIF1's atomic cutover
and deletion population; references to the earlier authority inventory's
`D12`–`D15`, catalog `CAT-F04` and TypeInfo `TIF-D01`–`TIF-D06` keep their
existing assignments, rather than granting new ownership here.

| Id | Displaced route / evidence | Replacement or structural rejection | Deletion owner; receiving acceptance |
| -- | -------------------------- | ----------------------------------- | ------------------------------------ |
| D01 | Component-meta-specific request/resolution orchestration in `resolver_core/component_meta_query_engine`, `meta_resolve`, host/session paths | OP10 view over shared dispatch facts; remove parallel metadata resolution, retain engine projectors/typed fact producers needed by the view | TIF1; `TIF1-AC1` |
| D02 | Component-specific result/payload/schema authority (`CachedComponentMetaResult`, `ResolutionTemplate`, component-meta result facade, encoded-payload admission) | View projections over engine-owned observations with one validity/admission story; retire metadata-specific authority and keys, not the generic engine stores or their fact-validation machinery | TIF1; `TIF1-AC1` |
| D03 | `FrameworkSurfaceStore`, Vue/Svelte surface keys, erased stores/bundles attached to `FrameworkRegistration` (`framework/surface_store.rs`, `registry.rs`) | Engine-owned TypeInfo observations; catalog rows carry no result cache/Any bridge | TIF1; `TIF1-AC1` |
| D04 | Macro-only `get_public_api*` and `ComponentPublicContract` as independent semantic assembly | Structured contract and declaration/public/testing output are projections of the same view/roles; transport/rendering may remain, semantic duplication cannot | TIF1; `TIF1-AC1` |
| D05 | Full/selective component-meta type graph/registry DTOs, `TypeHandle` vocabulary, legacy serde TypeInfo entries and FFI aliases | Typed TypeInfo request/result and NodeRef-based projections; old schema authority cannot remain active beside the replacement | TIF1; `TIF1-AC1` |
| D06 | JS compat/descriptor machinery capable of private type substitution, flattening or missing-meaning recovery | Mechanical projections of producer-backed roles; retain useful output equivalence, reject any independent semantic route | TIF1; `TIF1-AC1` |
| D07 | `ComponentContractEnvelope` as a second type graph | Forbidden design, not a live symbol at this head: repository lookup finds none. References use the TypeInfo graph, never a new envelope graph | TIF1; `TIF1-AC1` |

The shared `verter_type_engine` node/relation memo, typed lowering/replay,
`ProjectTypeStore`, source registry, parser inventories and certified provider
binding are retained authorities. Delete duplicate ownership, not those
producers. The per-request component scan keeps its IDX0 deletion owner
(`IDX0-AC1`); LSP/MCP framework-selection branches keep COX0 (`COX0-AC1`),
and coordinate conversion keeps ENCF0. Their product implementations are not
silently transferred to TIF1 by a consumer-adoption row.

## Incremental and cached authority

All demanded semantic facet facts participate through their TypeInfo child
observations, including membership, booleans/defaults, type roles, conditional
availability and provenance dependencies. Authored structure, imports/usages
and style links validate their existing producer's artifact/source basis;
they confer no permission to retain an incomplete wrapper. Optional capture,
display strings, compat policies and serialization layouts are not semantic
lookup material.

Candidate lookup uses `QueryIdentity`; each single-basis child flights on its
existing `SemanticFlightKey`. Concurrent compositions share only child flights.
Any composed cache record stays in `ProjectTypeStore`, scoped by descriptor,
child identities and each child's basis, isolated by profile/edit/evaluation
context (`TR28`, `TR32`). It cannot answer a child request. Warm reuse validates
the finalized read-set facts and self roots, adopts the signature in every
enclosing tracer, and validates certified TypeScript publication/serving fences.
Rejected candidates adopt no facts; cold production traces its actual reads,
including dependencies first loaded during the request.

Relevant owner/dependency/default/branch/style edits invalidate affected
observations; unrelated edits may reuse only validated unchanged facts.
Project/configuration/lib changes, profile/exact-release changes and generation
retirement follow their owning observation law. Source-linked observations
require the exact content and map revision; stale references fail rather than
being clamped/remapped. A backend/epoch/required-version mismatch is
`Unavailable(StaleBackend)`, never a warm answer.

Quality is independent of reuse. A producer-certified deterministic
Approximate fact is shareable only when its existing admission law permits it;
the view grants no new retention class. Budget/cancellation partial values and
ordinary incomplete/unavailable wrappers are return-only. Eligible refusals
reuse only in their existing certified/profile/isolated-root refusal class,
with original causes, recovery and dependency/accounting evidence. A retained
child refusal cannot admit a partial view. Same-profile replay stays refused;
higher allowance or an input repair can recompute and heal. Cancellation,
torn/superseded views and stale backend publish no fact or reusable refusal.
Required correctness state survives capture off.

TIF1 must compare edited incremental output with a fresh host for each changed
semantic dependency, while separately checking untouched sibling observations
and presentation-only changes. Equality compares semantic facts, child bases,
status/provenance and role references under the same current inputs; request
ids, optional traces and display choices are not semantic equality.

## Discriminating case table

Cases below are future executable obligations, not claims that this docs-only
change ran or introduced tests. Existing local examples are reuse candidates;
their legacy behavior must be compared at the actual cutover. Unless a row
names another owner, TIF1 receives it at `TIF1-AC-R1` (positive contract), with
`TIF1-AC3` for cache cases and `TIF1-AC1` for retirement. Test names should
describe the durable regression, not the case/plan identifiers.

| Case | Input/control | Required outcome / discriminating refusal | Existing evidence to reuse; receiving owner |
| ---- | ------------- | ------------------------------------------ | ----------------------------------------- |
| V01 | Every type lane in `F03`–`F09`, `F13`–`F15`, `F27`, including event/slot returns, nested parameters, registry, options/index signatures and public instance | Each resolves through the exact child/decl-slot/role; wrong child basis, composed-parent NodeRef and metadata-owned graph are refused | TypeInfo request/graph contract tests in `verter_protocol`; session output and framework transport equivalence tests; TIF1 |
| V02 | Same request under changed display/compat policy, schema/string-table layout, capture or requester encoding | Equal semantic query identity/facts/cache behavior; presentation bytes may differ. Compat performs no follow-up solving | `compat_one_napi_call_audit`, `packages/component-meta/src/compat/checker.spec.ts`, native projection specs; TIF1 |
| V03 | Healthy member beside failed member; independent required/default boolean and authored syntax | Preserve known membership, exact sibling and authored splice; failed value unavailable with original causes, not `unknown`/false/empty | Native facts/graph decoder specs and receiving fact contract; TIF1 |
| V04 | Complete empty, Complete open, Approximate open, optional absence and Unavailable with unknown cause | Closed empty alone proves absence; open domains prove positives; unfamiliar codes survive and never choose status | `component_facts.rs`, native-eval independent-domain case, schema-12/current decoding tests; TIF1 |
| V05 | Duplicate event names/overloads, same binding name in different slots, shuffled loads/queries | Occurrence/role identity retains every row; canonical equality deterministic, source presentation retained separately | `framework/public_contract.rs` and TypeInfo surface/transport tests; TIF1 |
| V06 | Vue destructure `zero = 0`, `verbose = false`, `empty = ''`, `label: text = 'hi'`; change/remove initializer | Keep prop `label`, defaults including falsy presence and value, requiredness; edited equals fresh native and compat | `native-eval.spec.ts`: reactive props destructure defaults case; carries ECRV3; TIF1 |
| V07 | Distinct slot binding names/types, same name across slots, optional and nested bindings | Binding names never become types; concrete leaves and truthful shallow carriers retained; native/compat agree; edited equals fresh | `native-eval.spec.ts`: concrete slot binding leaf types case; slot graph matrix/import tests; carries ECRV4; TIF1 |
| V08 | Svelte callback whose named symbol is callable, framework member with shallow/opaque payload, non-callable/ambiguous controls | Keep named-symbol observation distinct from member response; no promoted precision, name-prefix classifier or raw-text recovery; explicit path demand may resolve via its own observation | `typeinfo/framework_surface/svelte_exec_tests.rs`: callback named-ref, optional alias, two-callable-arm refusal and snippet cases; carries ECRS4; TIF1 |
| V09 | Imported generics/fallthrough across two parents and branches; edit producing dependency | Reference producing scope, profile, basis and origins, not parent scope; conditional members stay conditional; edited equals fresh | `component_meta_surface_equivalence_tests.rs`, fallthrough and publication-fence tests; TIF1 |
| V10 | Same-profile refusal repeat, higher allowance, root/context/entry-allowance change, missing-input repair | Original refusal/recovery on eligible repeat with zero producer executions; higher profile/repair heals; no cross-context reuse or partial wrapper admission | `component_meta_no_cache_promotion_tests.rs`, invalidation/refusal tests and receiving fact contract; TIF1 |
| V11 | Warm dependency validation, unrelated edit, source/map/project/backend change, cancellation during publication | Relevant change misses/refuses; valid unchanged child may remain warm; stale/aborted output never published; capture off preserves correctness | Warm invalidation oracle, concurrency, abort and publication-fence tests in `verter_session`; TIF1 |
| V12 | Preact function/class/generic/compat component; callback/children; requested Model/Options; unavailable provider operation | Preact-owned types, derived provenance, explicitly unsupported Model/Options and typed provider limitation, never Vue-shaped supported-empty | `docs/arch/framework-preact.md` and PRE3 corpus contract; PRE3, `PRE3-AC2`–`AC4` |
| V13 | CSS custom-property/part link, absent runtime/accessibility observation, style edit with unchanged input type | Link status/basis explicit; external observation cannot change type truth; style link invalidates on its own producer basis | Current style/structure facts for existing names; additional producer cases belong to their own consumers; TIF1 verifies view link isolation |
| V14 | Single, concurrent scalar and batch full/selective/audited requests on NAPI/WASM/LSP/MCP; genuine absence vs failed publication | Same facts and typed failures; batch order/per-item behavior retained; one view/authority, no null/error conflation | `framework/transport_route_equivalence_tests.rs`, component-meta batch/native specs, custom LSP tests; TIF1 |
| V15 | Multi-payload emits, nested full-API payload and PropType factory defaults | Parameter optionality, second-parameter structural members and nested value/size/filters/pagination/scope types survive; no display-string oracle | `native-eval.spec.ts`: nested/optional multi-payload and PropType factory-default cases; TIF1 |
| V16 | Old entrypoints/schema paths after atomic adoption of `C01`–`C16` | Thin projections only where retained; no independent resolver/store/schema/type-graph authority or arbitrary new raw cache access | Retirement rows `D01`–`D07`; TIF1 |

## Acceptance and evidence selection

- **TIF1D-AC1:** `F01`–`F27`, `C01`–`C16` and `D01`–`D07` give each populated
  boundary one production adoption/deletion owner and receiving acceptance.
  UAO0's validator (`UAO0-AC-R1`) receives these tables; it must reject an
  omitted field/consumer/route, conflicting owners, non-production deletion
  owner, or missing acceptance. No validator ships here.
- **TIF1D-AC2:** the logical schema binds OP10/NodeRef/child bases, provenance,
  independent guarantees and canonical ordering to TIF0 and the current
  per-fact transport. `V01`–`V08`, `V14`–`V16` carry the concrete cutover proof,
  including the three inherited regressions. Existing tests are cited, not
  certified as passes by this document.
- **TIF1D-AC3:** the invalidation/admission section and `V09`–`V11` specify
  incremental equivalence; TIF1 supplies executable proof. This document
  changes no cache or publication path.
- **TIF1D-AC4:** runtime work, allocation and latency measurement are not
  applicable: zero production code, no new hot path, parser/solver call,
  store, timer or counter. At implementation, demand-selected work and
  existing read/resolve/audit counters verify no duplicate resolution; do
  not manufacture a benchmark for this decision.
- **TIF1D-AC5:** unsupported applicability, conditional predicates, absent vs
  undemanded facts and explicit link status are specified above and exercised
  by `V03`, `V04`, `V09`, `V12`, `V13`. The current three-arm fact protocol
  with open causes can carry these statuses; no consumer in the inventory
  requires undecidable semantic/presentation classification.

TIF1 must reconcile these exact source paths with any intervening owner
extractions, preserve the receiving guarantees and update the owning
component-meta reference at its atomic cutover. This decision adds no new
enforcement machinery or product promotion claim. Independent architecture
review and the workflow's final verification judge this contract on its
current content, not on a historical Git object or stored transcript.
