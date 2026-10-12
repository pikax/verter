# Native metadata transport inventory

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Macro argument surface claim | Session normalization preserves closed/open/approximate/unavailable domains before Option adoption | REQUIRED | Existing demand and DTO production scope | project_semantic_dispatch/one_level_surface.rs | always |

## verter_protocol

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| FfiFact, FactCauses, FfiComponentFacts and boolean facts | Native consumers distinguish availability, approximation, false and absence | REQUIRED | Borrowed terminal output, released after serialization | component_facts.rs | always |
| FactGuarantee, open cause codes, delivery disposition and ComponentFacts | Protobuf and JSON decoders preserve independent guarantees and refusals | REQUIRED | One transport payload | proto/verter/v1/component_meta.proto, component_meta.rs | always |
| Infer constraint, GraphNode and persisted graph constraint edges | Binder identity, graph validation and rendering | REQUIRED | Terminal graph or existing persisted graph owner | graph/builder.rs, graph/snapshot.rs | always |
| Derived result completeness and compatibility projections | Legacy readers display aggregate partiality without authorizing individual facts | REQUIRED | One transport payload | component_facts.rs, component_meta.rs | always |

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| MetadataFactGuarantee, ComponentDomainGuarantees, analysis and expanded-shape mirrors | Output projection preserves the producer membership guarantee without inspecting ambient sibling status | REQUIRED | Existing shallow analysis, expansion and result owners | analysis/component_meta.rs, analysis/type_expand/request.rs | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| TypeInfoSurface and MacroSurfaceDtos domain guarantees | Normalization and terminal publication preserve closed/open membership and missing evidence | REQUIRED | Existing surface and request/result owners | typeinfo/surface.rs, typeinfo/surface_resolution.rs, typeinfo/framework_surface | always |
| Terminal materialization completeness scope | Encoded-payload admission refuses partial success caching; domain guarantees come directly from the producer claim | REQUIRED | Request-local scope | meta_resolve/projectors/output_sink/envelope.rs | always |
| Materialized publication failures and envelope completeness accessor | FFI conversion and payload cache admission preserve unavailable values and execution aborts | REQUIRED | Consumed terminal envelope | meta_resolve/output.rs, meta/output_api.rs | always |

## verter_ffi

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Domain guarantee transfer and optional event payload | Native transport preserves the same fact and carries no unavailable value | REQUIRED | Destructive terminal transfer | convert/component_meta.rs | always |

## TypeScript transport

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| FactResult, nonempty causes, member boolean/value facts and NativeComponentFacts | Public readers guard exact claims and distinguish absence from unavailability | REQUIRED | Decoded component result | packages/component-meta/src/facts.ts, native-component-meta.ts | always |
| Invalid-node set and reverse dependency closure | Localized graph validation refuses only dependent values | REQUIRED | Decoded graph owner | packages/component-meta/src/type-graph-core.ts, type-graph-proto-decode.ts | always |
| Legacy decoder and conservative schema-12 adaptation | Older payloads remain usable without introducing exact negative claims from partial arrays | REQUIRED | One decode | packages/proto/src/component-meta.ts, packages/component-meta/src/type-graph-proto-decode.ts | always |
