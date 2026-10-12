# Native component facts

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
Output-only numeric mirrors travel with shallow metadata and never drive
semantic evaluation. Missing evidence refuses a guarantee. Materialization
failure of a published member produces a failed publication with no type value;
other members still publish. Execution aborts still publish nothing. Terminal
partiality also refuses encoded-payload success admission.

`@verter/component-meta` exposes `FactResult`, `FactCause`, `FactCauses` and
`NativeComponentFacts`. Native `ComponentMeta` includes the facts. Compatibility
arrays are derived projections and confer no independent completeness claim.
The JSON serializer uses the same borrowed fact projection as protobuf; it owns
no parallel member store. Refusal delivery carries its original machine causes
independently of recovery facets. Same-profile replay does not become complete;
a larger profile or an edit can recompute and heal.

Schema-12 decoding uses a separate legacy carrier. A partial legacy summary
cannot prove a closed domain: its arrays adapt conservatively with a legacy
partiality cause. Current decoding ignores an inconsistent legacy summary and
derives the summary from published facts. Graph validation marks invalid nodes
and their reverse dependency closure once; a bad member value does not erase
an independently decoded sibling or the known member domain.

Constrained `infer` nodes retain both name and constraint through graph
interning, persisted graph validation, protobuf and TypeScript rendering.
Optional capture still contributes no origin graph by default. No new counters,
histories, timers or optional observation stores are introduced.
