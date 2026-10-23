# Parse artifact storage accounting

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The fields and hooks below are REQUIRED policy accounting. They maintain
current live storage, never capture history, and compile with optional capture
off. Nothing here refuses computation, evicts a live caller's artifact, or
changes answer production. All production contexts use the one process-local
retention account. Isolated contexts exist only under test support.

## Measurement and ownership conventions

The heap-block, Arc-block, capacity, inline-field and owned-cell conventions
are those of [the common inventory](SKR-OWN-ACCOUNT.md). `HeapSize` derives
destructure every field of every variant; adding an owning field requires
measurement at compile time. Copy-only fields own no separate heap. No source,
analysis, declaration, inventory or parser payload uses a fixed per-item weight.

`PinContext` shares one process `AliasCharges` registry. A pinned measurement
registers each distinct raw Arc allocation once, including its variable heap;
independently published artifact and scheduler owners share that registration.
The registration co-owns the allocation, preventing address reuse while any
holder references its charge. An owner's hold list pays for its own backing.
Nested `Owned` cells pin themselves, so their bytes are excluded from parent
measurements. A shared allocation has one charge until the last registered
owner releases it. Raw Arcs are artifact-internal data. Records cloned out of
publication cells (indexed/shallow inputs, lowered declarations and registered
carrier envelopes) embed their own pin, so an escaping reader retains the
charge until its last release. The cell's adjacent pin covers only backing
that cannot escape independently.

`StoragePin` lives inside its owning record, including when that record is
shared through Arc. Its root block and unique backing use `GranularPin` with
1 KiB granules and the existing less-than-two-granule shrink convention.
Aliased raw allocations use the registry's ordinary exact reservation and
backing convention. One iterative worklist attributes deferred nodes to their
owning Arc before settling registrations; crossing an Arc does not start a
recursive measurement. `PinnedOnce` publishes a value and its pin together under
its write-once cell; a losing setter drops its provisional pin. A mutable
shallow assembly invalidates its old measurement before mutation and is pinned
again before publication. Clone resets assembly pins; it shares child charges
and measures the clone's independently allocated backing when published.

## Allocation and field owners

| Owner / fields | Byte computation | Sharing and release |
| --- | --- | --- |
| `IndexedReady.storage` | Arc block of the complete record; its measured fields below | Embedded in the artifact, retained by all artifact readers |
| `IndexedReady.raw_source`, `eval_source` | Each distinct Arc string's byte length and Arc block | Equal raw/eval allocations and scheduler aliases register once |
| `IndexedReady.file_language`, `framework_parse`, `script_analysis`, `export_signatures`, `snapshot`, `route_inventory` | Exhaustive recursive measurements; vector/string/map capacities and nested allocations | Registry-shared raw Arcs; `SharedList` and template-analysis owned cells charge themselves |
| `IndexedReady.source_parse_key`, `input_projection` | Charged write-once canonical identity vectors and self-pinned indexed-input records | Lazy; identity storage releases with its cell, while input readers can outlive the projection; no eager hash or projection added |
| `IndexedInputRecord.storage`, `ShallowInputRecord.storage` | Complete root and exhaustively measured fields; the indexed record's `source_parse_identity` has its own charged write-once cell | Input readers keep their pins after artifact/projection teardown; lazy identity initialization settles immediately and warm reads do not recharge |
| `FileArtifacts.storage`, `facts`, `augmentations` | Complete root block; `FileFacts` registry maps, cached export fact, lazy source; augmentation vector capacity and all fact/key fields | Payload pins survive store eviction while an artifact reader holds the record |
| `LazyBodyFactSource.memo`, `lens`, `synthesised_value_bodies`, `computed` | Shared memo/lens pins, owned map capacity, every synthesized declaration, charged per-file map entries | The source owns its copies of map backing; shared declarations and lenses register once |
| `ShallowFileState.storage`, `inputs`, `synthesised_value_bodies` | Complete root block; all `ShallowInputAssembly` fields (routing maps/sets, export assignment, canonical/snapshot identity, route/header/owner inventories, synthesized symbols), every synthesized body | One embedded pin shared by artifact and both prepared declaration caches; no bundle-side recount |
| `ShallowFileState.type_deps_cache`, `input_projection` | Charged per-file entries and shard capacities; charged write-once shallow-input projection | Lazy growth and last-owner teardown; digest cells contain inline hashes only |
| `PreparedDeclBundle` and `Prepared*DeclCache.state` | Inline handle only; the state owns the complete charge | Either release order retains exactly one state charge until the last holder drops |
| `DeclBodyMemo.storage`, `key`, `framework_parse`, `owner_table`, `snapshot`, `header_index`, `function_index` | Complete memo root; snapshot-key text, owned framework facts, shared owner/header tables, snapshot-demand and function-index-demand blocks | Registry aliases share headers/owners/source with the shallow state; retained parser leases keep their existing separate charge |
| `DeclBodyMemo.lens`, `route_lens`, `whole_env`, `FunctionIndexDemand.function_program_index` | `PinnedOnce` publication of every map, vector, locator, capture summary and class-index field | Demand-only; initialized once, released with the memo's owning cells |
| `DeclBodyMemo.type_entries`, `value_entries`, `aug_type_entries`, `aug_value_entries`, `raw_surfaces` | Key and committed-value footprints; lower-declaration facts, raw-surface fields, allocation capacity | A `PinnedOnce` body adds storage on publication; a warm read adds no charge |
| `LoweredTypeDecl.storage`, `LoweredValueDecl.storage` | Complete Arc root and every fact field through exhaustive derives | Embedded `FactStoragePin` accepts only transitive `NoTypeExpr` carriers; a declaration reader outliving the memo keeps exactly one payload charge |
| `PerFileMap` backing / entries | Actual padded shard-array size and each affected shard's bucket/control capacity; each key/value's complete heap and hold-list backing | Shards remain unallocated until first write. Entry guards release their shard before settling that shard's capacity; replacement/removal drops the displaced pin. Heap-owning values change through charged replacement; only Copy values expose mutable guard access |
| `DeclBodyMemo.primed_type_parts` buffer | Reserved Vec capacity times the complete entry size, rounded as a heap block | Transient bodies leave with the batch guard; only reusable buffer capacity remains charged until memo drop |
| `FileArtifactStore` live/retired artifact, canonical-key and augmentation tables | Actual shard arrays, bucket arrays and control bytes by capacity; capacity-tracker arrays; stored key canonical/parse identity/language fields | Only a touched shard settles after its guard drops. DashMap removal retains bucket capacity; monotonic capacity maxima prevent a racing older observation from reducing the charge. No unrelated shard or payload scan |
| `PinnedChain` canonical/retired memberships | Actual stored map-key footprint, every version's payload/key footprint, spilled value/pin-list capacities | Retired roots retain their charges until reclamation. Retention and buffer capacity are independent |
| `StoredArtifact.key_storage`, `AugmenterVersion.storage` | Stored key footprint and shared augmenter-set data (exact artifact keys and all contribution fields) | Each independently allocated key buffer is charged; canonical text and sets share registrations |
| Artifact freshness leases | Lease Arc block | Workspace history remains owned by its separate workspace authority; a lease adds no second history copy |
| `MacroHotMirror.cells`, `MacroSlot`, committed value and `MacroHotProduct` | Mirror cell block; lazy slot-array capacity; committed outer Arc; product root block and property-reference fact lists | Slots and products charge on first demand/commit. Existing graph root owners keep their own charges and graph lifetime rules |
| `HostSourceData.storage`, source snapshot raw source | Complete data root, parse snapshot fields below, raw/eval source allocations, framework artifact, parse key, registered file envelope and source identity, language fields | Shares raw source, framework parse, script analysis and identities with the indexed artifact |
| `ParseSnapshot` | All descriptor fingerprint vectors, file metadata strings/vectors, external-source records, src-block records, diagnostics and argument text, script analysis, export signatures, styles, prepared style IRs, markup tokens, preprocessor requests | Actual capacities, exhaustive field measurement; hashes, spans and refusal flags inline |
| `HostAnalysisData.storage`, `AnalysisArcs` | Complete root, script-analysis allocation, export-signature Vec and every shared analysis list | Source/artifact aliases share script analysis; independently cloned export signatures pay their own capacity; `SharedList` cells charge once |
| `FileAnalysisSnapshot` | All fields through its exhaustive derive, including mutable imports/bindings, macro usage, style roots, options API and revision | Raw snapshot Arc shared by retaining owners; nested lists/template owned cells preserve their existing lifetime charges |

`HeapMeter.pending`, `allocation` and `allocations` are transient REQUIRED
accounting traversal state, discarded before publication. `EmbeddedStorage`
and the derive's pin hook route REQUIRED accounting to the record's single
embedded owner. The scoped default-context override and the executor's
`pin_context` field are test-support injection only, absent from shipped
builds; they add no production observation or retained store.

## Compiler and parser records

Compiler, parser, identity, CSS and language records expose only pure heap
measurement. They do not mint accounts or make admission decisions.
`CarrierParse` requires measurement, preserving it through erasure without a
carrier downcast or an unmeasured default for future adapters.

| Payload | Measurement |
| --- | --- |
| `FrameworkParseArtifact`, registered carrier payload/inner, `FrameworkArtifactEnvelope.storage` | Every identity, canonical byte-vector capacity, common geometry, diagnostic field, carrier block and Arc; marker geometry and hashes inline. The envelope embeds its complete root pin |
| `CarrierBlockInventory` | Source-space, normalized-name, block and markup Arc arrays, source bytes, every nested attribute, directive modifier, mixed-value part, markup/control node and identity |
| Vue carrier | Parsed SFC plus parse-option delimiter strings/custom-element vector and strings |
| Svelte carrier | Parsed Svelte tree plus the separate attribute-expression record vector and every indexed expression |
| Parsed SFC / Svelte | Parser-owned `retained_bytes()` capacity walkers as an opaque payload convention; subtract inline record bytes, which ride its Arc block. These walkers cover arena/buffer reservations and recursively owned parser records; source bytes are separate shared allocations |
| Prepared styles | Both shared IR blocks, shared CSS source, diagnostic/dependency/comment vectors, frozen bump reservation including spare capacity and bump metadata. One aggregate arena block is the declared convention; bump-backed child strings/slices own no additional blocks |

## Behavioral evidence

Default-build tests cover deliberately oversized aliased sources, spare string
capacity, nonempty nested payloads, shared-owner release orders, prepared
bundles outliving artifacts, lazy declaration growth/warm reuse, replacement,
removal and zero-headroom pinned publication. Isolated contexts scope default
pins on the test thread and explicitly propagate to host stage workers; tests
remain independent under concurrent libtest execution. Source/analysis stages
run with oversized carrier source, nonempty exports and style analysis; an
indexed artifact shares their source charge. LRU eviction retains charged
versions under captured roots until reclamation. Host close with demanded
declarations and a prepared bundle releases all payloads after final readers
drop. Mutation experiments remove a source charge and
duplicate a shared charge; the corresponding behavioral tests reject them.
Compile contracts and architecture checks preserve the inactive prepared
compiler production boundary and the existing semantic capability seals.
