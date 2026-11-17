# Native component facts

`verter_ffi::component_meta` owns the materialized-payload byte encoder and
the shared `AuditBundle` with its JSON byte encoder. The bundle has exactly `analysis` and `record`; resolution appears only at `analysis.resolution`. NAPI and WASM scalar and
batch component-meta calls use the seeded session payload entries and the same
encoder: NAPI returns Buffers, WASM returns `Uint8Array | null` slots. WASM's
audit entry returns the shared bundle's UTF-8 JSON as text; `decodeAuditBundle`
parses it, and both provenance helpers forward the original text to Rust.
WASM supports standalone projects only and runs batches inline. Audit capture
is unavailable without `semantic-observe`. `verter_ffi::typeinfo` owns the shared
typeinfo request helpers, audited execution, response encoding and stored-record
gate. Binding error mapping preserves native argument/serialization statuses
and WASM string errors. Response-byte attribution fires once in the shared
encoder; its observation gate is owned by `verter_audit`.

JavaScript consumers decode scalar WASM metadata bytes and each non-null batch
slot with `decodeComponentMetaPayload` from `@verter/component-meta/browser`.
The same decoder and its `NativeComponentMetaResult` return type are also
exported by `@verter/component-meta`. The browser entry re-exports the shared
decoder directly; its native-result imports are type-only.

`component_meta.proto` defines one schema, versioned by the single
`COMPONENT_META_SCHEMA_VERSION` constant (currently 15). Protobuf and the JSON
rendering both state that version (`schemaVersion` in JSON). JSON has no
`resolutionStatus` field. Adding JSON's version and removing that field do not
bump the unchanged protobuf schema. Proto field additions, removals, retyping
or meaning changes require a bump; producer claim changes and new open cause
codes do not.

Schema 13 gives `ComponentMetaBody.facts` (field 28) ownership of semantic lanes.
The former body arrays and contract fields are reserved, as is the former
payload registry field. Field 27 carries the envelope's measured completeness,
as does JSON `resultCompleteness`; neither derives it from independent facts.
`SurfacePartialReason` codes 1–17 retain their meanings. `FactCause` is an open
numeric vocabulary: a reader retains unfamiliar codes and never uses them to
choose availability.

A fact is Complete with a value, Approximate with a value and nonempty causes,
or Unavailable with causes and no value. Complete optional absence and false
remain distinct from an unavailable answer. A complete open member domain
proves positive members only. An approximate domain is a lower-bound enumeration.
Member publication and boolean facts remain independent of membership. Event
and slot occurrences retain source order, including duplicate names/overloads.

The session preserves the membership guarantee before member normalization.
The macro producer returns its typed surface claim through DTO construction,
including complete no-surface and unavailable answers. A completeness summary
never reconstructs this guarantee, and a second surface probe is unnecessary.
Vue props, events, slots, models and exposed domains read that claim from the
framework-surface bundle, never from expanded `define_*` shape rows. Runtime
props, emits and expose bundles use the analyzer's enumeration certificate:
complete/closed when fully enumerated, otherwise approximate/open with cause
103. Runtime emits arrays and objects publish their captured event names through
the shared runtime-object surface synthesis used by props and expose. A literal
`defineEmits(['saved'])` therefore reports Supported with the `saved` member;
an identifier argument or spread reports Partial and retains only captured
literal members. A runtime `defineSlots()` without a type argument certifies
nothing: its bundle states approximate/open with cause 103 and its empty
framework surface reports Partial. Component meta reads these same bundle
claims. A runtime macro without a bundle claim uses the analyzer certificate
under the same rule. The framework-surface operation preserves a bundle's
approximate membership as a partial surface status.
`ExpandedObjectShape.domain_guarantee` remains an output-only mirror for the
public `evaluateTypes` JSON response; it does not authorize component facts.
Output-only numeric mirrors travel with shallow metadata and never drive
semantic evaluation. Missing evidence refuses a guarantee. Materialization
failure of a published member produces a failed publication with no type value;
other members still publish. Execution aborts still publish nothing. Terminal
partiality also refuses encoded-payload success admission.

Resolve and extract each measure completeness in their own
`ColdComputeCompletenessScope`. Resolve merges its scope's observations with
the synthesis claim, then discards the scope; extraction likewise carries its
measured claim with the outcome. The request-wide sticky flag is diagnostic
accumulation, never an individual component's completeness authority: an
earlier partial resolution in the same request must not prevent a healthy
later component from warming.
Consumers that build a cached product from a resolved state must fold its
carried completeness into that product's active compute scope. Fallthrough
does this both after its own owner resolve and inside the fresh scope used
when extraction supplies an already-resolved state. The surface walk need
not repeat the resolve-only work that produced a partial claim.
Field-only evaluated-type consumers use
`ResolvedComponentMetaState::into_evaluated_types`: it folds the carried claim
before moving out the types. Both the base `evaluate_types` entry and the
view-aware `evaluate_types_via_view` entry use this handoff. Wrappers returning
the full resolved state retain completeness explicitly with the products.

`@verter/component-meta` exposes `FactResult`, `FactCause`, `FactCauses` and
`NativeComponentFacts`. Native `ComponentMeta` includes the facts. Compatibility
arrays are derived projections and confer no independent completeness claim.
Public descriptor arrays retain every known member, including unavailable values.
Each prop, event payload, slot binding, model, exposed/public-instance member,
accepted member and fallthrough member has a `typeFact` transcribed to descriptors.
Its `type` or `payload` is absent for an unavailable value. Complete optional
absence remains a complete fact with an undefined value and the wire publication's
`absence` reason (`unannotated`, `branchDivergent` or `unclassifiedConstructor`).
An Absent/Unannotated publication decodes with no value even when the wire
attaches a graph node: that node is the output sink's missing-output placeholder,
not a type descriptor. Other absence reasons preserve a supplied descriptor.
Runtime constructor annotations outside the closed primitive classification
publish `UnclassifiedConstructor` (wire absence 3), never `Unannotated`.
The shared constructor helper applies this to Options API props, runtime
`defineProps` objects and constructor arrays. Local binding resolution retains
its existing evaluated source or typed failure; no instance-type resolver is
inferred from display text. Array-form prop names such as `props: ['x']`
remain Unannotated.
Unavailable semantic
lanes are absent on both decoded native results and public `ComponentMeta`;
their guarantees and original causes remain on `facts`. Event rows carry no
`hasValidator` or `isDeclared` flags. Healthy siblings remain usable.
Compat omits members without a usable descriptor or an explicitly unannotated
type fact (props also need a known requiredness boolean). Only a complete fact
with no value and `absence: "unannotated"` proves absence of an authored type:
TypeScript treats it as `any`. Other absence reasons never authorize this mapping;
descriptors for other publications, including structured `unknown`, pass through
unchanged. Unclassified constructors therefore keep their existing compat
`unknown` rendering (with `| undefined` when optional).
`compat/checker.ts::unannotatedTypeAsAny` applies the scalar rule to props,
slot bindings and exposed members. `unannotatedEventPayloadAsAnyArray` maps
unannotated event payloads to variadic `any[]`, including runtime array emits.
Compat represents this with a rest-array tuple, classified as an argument array
for the existing display and schema renderers. An untyped model's update event
also renders `any[]`: the decoded event row carries no typed model-provenance
fact authorizing a single-value tuple. A producer-owned model-provenance fact
on the wire would permit that distinction; an `update:` name never proves it.
Unannotated native rows without a descriptor keep their complete-absent
`typeFact` and have no `type` or `payload` key; compat never mutates them.
The JSON serializer uses the same borrowed fact projection as protobuf; it owns
no parallel member store. Refusal delivery carries its original machine causes
independently of recovery facets. Same-profile replay does not become complete;
a larger profile or an edit can recompute and heal.

The decoder accepts exactly schema 15 and rejects schema 12. The legacy proto
messages remain Rust encoder build intermediates, not a readable JS transport.
The decoder reads measured completeness from body field 27 and root info from
field 21. Unknown closed summary reasons fail decoding. Accepted-surface
completeness alone is a JS projection: `projectAcceptedSurfaceCompleteness`
reads the accepted-props guarantee, shared by accepted events and fallthrough.
Only `ComponentMetaGraphError` can be localized; programming errors propagate.
A localized bad publication uses the existing publication-failure cause 102;
a bad direct graph reference uses missing-semantic-node cause 11. Invalid
envelope facts preserve nonempty wire causes or raise an error when none exist.
The decoder owns no cause-100 sentinel. Graph validation marks invalid nodes
and their reverse dependency closure once; a bad member value does not erase
an independently decoded sibling or the known member domain.

Constrained `infer` nodes retain both name and constraint through graph
interning, persisted graph validation, protobuf and TypeScript rendering.
Tuple descriptors retain the wire's readonly flag and aligned per-element
labels, optional flags and rest flags. `@verter/type-ir` represents these as
`readonly?: true` and `modifiers?: { label?: string; optional?: true; rest?: true }[]`
beside `elements`; absent modifiers mean every element is plain. A rest
element retains its complete type, such as `boolean[]`. The `tuple` builder
accepts elements, optional modifiers and a readonly boolean. Native JSON and
both graph decoders preserve these facts, and descriptor-to-native conversion
round-trips them. Descriptor consumers use `modifiers`, replacing `labels`.
Optional capture still contributes no origin graph by default. No new counters,
histories, timers or optional observation stores are introduced.

Partial component reads use the framework-surface operation through
`typeInfoWithAudit` and expand-around on a `NodeRef`. The full component-meta
payload remains the composite metadata entry.

Every variable component domain retains its producer claim on
`ComponentDomainGuarantees`: props, events, slots, models, exposed, accepted,
public instance, and type registry. Model membership proves the syntactic
names; an unresolved model value is independently unavailable. Type-based
expose preserves the bundle's openness and availability; object-literal
expose states its analyzer-owned complete closed names only when every literal
member has a static name. A spread, computed key, or another unnameable key
withholds the runtime enumeration certificate and makes analyzer-owned exposed
membership
approximate/open with cause 103, while preserving known members. A present
non-object expose argument likewise withholds enumeration; an absent
argument certifies an empty domain. Type-based expose keeps the resolved
surface producer's claim, including when a runtime argument is present. Accepted props, events
and fallthrough share the resolver claim and absorb surrounding partial-compute
causes. Public-instance completeness stays typed through the wire DTO.

A transport registry carries the resolution policy's completeness. Internal
sidecar-less output readers still use the shared envelope builder; they are
not the WASM transport. The output sink finalizes the
registry and emits aligned declaration positions, including absent declarations.
The converter zips these positions and copies rawType from that same declaration.
The cause-code vocabulary is defined once by
`metadata_partial_reason_causes!` in `verter_session_query`; the session's
forward projection and protocol's forward projection expand that table.
The terminal output sink folds failed publications and localized unraisable
exposed/public-instance members into measured completeness with a missing-source
cause. The semantic extractor carries typed source failures to that session
boundary without depending on its request context. Such partial outputs never
enter the encoded-payload cache. A localized projected-member miss keeps its
semantic-query-fault cause. Independent approximate domains, including the
public-instance lower bound, do not add reasons to this summary. This summary
never authorizes a domain; TypeScript decodes that measured value unchanged.

Public-instance exactness is finalized per delivery at the terminal output
sink, after it merges the measured completeness with materialization failures.
Each input domain positively reports fully enumerated membership through
`ComponentDomainGuarantees::enumeration`: props, emits, slots, exposed and
models. Missing reports default to false. The existing macro inventory fold
certifies absent domains; runtime props, emits and expose extractors record
`AnalyzedMacro::runtime_members_fully_enumerated` during their existing
member traversal. Literal objects with static names and literal name arrays
can certify enumeration; an omitted member or non-literal input cannot.
Calls to `defineProps()`, `defineEmits()` and `defineExpose()` with neither a
runtime argument nor a type argument certify empty, closed domains.
Type-based domains use their existing complete, closed producer claims.
Type-based expose retains the resolved surface producer's authority.
The model producer withholds its certificate because the sidecar lists model
value props but omits their modifiers props.
The slots fold also reads the template's existing `has_dynamic_slot_outlet`
report: a dynamic-name outlet withholds slot enumeration, including when no
`defineSlots` macro is present. Static-name outlets retain their enumeration.

The sidecar builder records only registered script-setup geometry and the
positive `options_member_free` geometry report. The existing macro-flags fold
certifies that report only without an Options API default export and when
every `defineOptions` call reports member-free arguments. Default-export
identifiers follow the root binding index to their local initializer; Vue's
`defineComponent` is recognized by its imported API identity, including aliases.
An unresolved default export in a plain script withholds the member-free report
and reports `OptionsApiMembers`; it never certifies an empty instance.
The `defineOptions` extractor uses its existing property traversal and the fixed
key set `name`, `inheritAttrs`;
every argument must be an object literal containing only non-computed keys
from that set. A spread, any other key or a non-literal argument withholds the
report. The terminal sink alone reads all five
certificates and domain claims together with that geometry and the merged
measure. Only a complete delivery with every certificate and a complete,
closed domain claim becomes Exact. Otherwise the instance stays Partial with
approximation cause 103, or the failed domain producer's more specific causes;
measured partiality is merged into the same lane. Plain literal runtime
`defineProps({ label: String, size: Number })` is eligible. The props-domain
runtime claim uses the analyzer's enumeration evidence; published member
value types remain independent.
The cached sidecar stays Partial; finalization operates on the delivery clone.
The fixed Vue built-ins excluded from this claim are enumerated beside the
sidecar builder; a test renders the real IDE public-constructor projection and
checks its omitted and retained built-in keys against that list and the
sidecar's own members, then checks every fixed-key exclusion is a real Vue built-in
omitted or retained by the constructor. `$slots` is excluded only when the
sidecar has no slots; otherwise it is listed as a sidecar member. The constant
and its re-export are test-only; production inserts `$slots` directly when
slots exist.

The closed public-instance claim also excludes global members contributed by
`ComponentCustomProperties`. Like the fixed built-ins, that augmentation class
is outside the claim's domain. Its members are not modeled, and this exclusion
adds no augmentation read or dependency.

Both public package entries re-export all of `@verter/type-ir`. The browser
entry also exports `nativeComponentMetaToComponentMeta`; its display component
name is derived from the final file path segment without Node APIs. Conversion
consumes decoded protobuf results, whose member value facts are required.
Structured v-model usage entries transcribe the `vModels` wire name list.
A richer structured v-model wire entry belongs to the schema-16 change.
