# Native component facts

`verter_ffi::component_meta` owns the materialized-payload byte encoder and
the shared `AuditBundle` with its JSON byte encoder. Bindings adapt these to
their boundary types; WASM's component-meta entries still serialize the shared
bundle directly to a JavaScript object. `verter_ffi::typeinfo` owns the shared
typeinfo request helpers, audited execution, response encoding and stored-record
gate. Binding error mapping preserves native argument/serialization statuses
and WASM string errors. Response-byte attribution fires once in the shared
encoder; its observation gate is owned by `verter_audit`.

Schema 13 gives `ComponentMetaBody.facts` (field 28) ownership of semantic lanes.
The former body arrays and contract fields are reserved, as is the former
payload registry field. Field 27 remains a derived compatibility summary.
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

Every variable component domain retains its producer claim on
`ComponentDomainGuarantees`: props, events, slots, models, exposed, accepted,
public instance, and type registry. Model membership proves the syntactic
names; an unresolved model value is independently unavailable. Type-based
expose preserves the bundle's openness and availability; object-literal
expose states its analyzer-owned complete closed names. Accepted props, events
and fallthrough share the resolver claim and absorb surrounding partial-compute
causes. Public-instance completeness stays typed through the wire DTO.

A seeded registry carries the resolution policy's completeness; an unseeded
registry retains its complete closed claim. The output sink finalizes the
registry and emits aligned declaration positions, including absent declarations.
The converter zips these positions and copies rawType from that same declaration.
The cause-code vocabulary is defined once by
`metadata_partial_reason_causes!` in `verter_session_query`; the session's
forward projection and protocol's forward/reverse projections expand that table.
The compatibility summary remains derived from independent facts and failed
publications: measured completeness alone does not yet cover every failed
publication. This summary never authorizes a domain.
