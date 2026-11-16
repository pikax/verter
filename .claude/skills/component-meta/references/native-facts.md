# Native component facts

`verter_ffi::component_meta` owns the materialized-payload byte encoder and
the shared `AuditBundle` with its JSON byte encoder. NAPI and WASM scalar and
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

`@verter/component-meta` exposes `FactResult`, `FactCause`, `FactCauses` and
`NativeComponentFacts`. Native `ComponentMeta` includes the facts. Compatibility
arrays are derived projections and confer no independent completeness claim.
Public descriptor arrays omit explicitly unavailable member values while the
facts retain their membership and causes; healthy siblings remain usable.
The JSON serializer uses the same borrowed fact projection as protobuf; it owns
no parallel member store. Refusal delivery carries its original machine causes
independently of recovery facets. Same-profile replay does not become complete;
a larger profile or an edit can recompute and heal.

Schema-12 decoding uses a separate legacy carrier. Its summary cannot prove a
closed domain: arrays retain open membership even when the summary is complete.
Partial arrays adapt conservatively with a legacy partiality cause.
Current decoding ignores an inconsistent legacy summary and
derives the summary from published facts. Graph validation marks invalid nodes
and their reverse dependency closure once; a bad member value does not erase
an independently decoded sibling or the known member domain.

Constrained `infer` nodes retain both name and constraint through graph
interning, persisted graph validation, protobuf and TypeScript rendering.
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
never authorizes a domain; current TypeScript decoding still derives its own
compatibility summary from the independent facts.

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
certifies that report only without an export-default options object and when
every `defineOptions` call reports member-free arguments. The options extractor
uses its existing property traversal and the fixed key set `name`, `inheritAttrs`;
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
sidecar's own members, then checks every exclusion is a real Vue built-in
omitted or retained by the constructor. `$slots` is excluded only when the
sidecar has no slots; otherwise it is listed as a sidecar member. The constant
and its re-export are test-only; production inserts `$slots` directly when
slots exist.
