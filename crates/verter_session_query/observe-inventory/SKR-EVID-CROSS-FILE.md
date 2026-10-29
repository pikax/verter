# Cross-file declaration evidence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## Route selection

| item | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ShallowInputAssembly::declares_local_binding_in` | Local export leaf selection | REQUIRED | Reads effective type/value presence and namespace/alias headers of one served snapshot; retains nothing | `verter_session_query/src/inputs/shallow.rs` | always |
| Exact-owner declaration-existence predicate on a reexport leaf | Imported-root fast-path selection | REQUIRED | Point lookup in the exact served shallow snapshot; retains nothing | `verter_session/src/host_manage/prepared_decl.rs` | always |
| Exact-owner declaration-existence predicate on a local export | Named export route walk | REQUIRED | Point lookup in the exact served shallow snapshot; retains nothing | `verter_session/src/host_resolve/frontier_engine.rs` | always |

The existence predicates establish only that the
selected lexical owner declares the binding; they do not certify declaration
content, member values or diagnostic independence.

The source-header shortcut also requires a unique export occurrence contained
in the selected declaration's span. This REQUIRED proof reads the two existing
SFC-absolute spans from one parser snapshot; it adds no stored identity or
instrumentation. Local export specifiers and ambiguous export occurrences use
the canonical shallow route table instead.

## Source-header proof

| item | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FactKey::DirectDeclarationExport` and its expected hash | Direct imported-root route; parse-fact validation without indexing | REQUIRED | The enclosing immutable fact signature | `verter_session_query/src/facts/registry.rs` | always |
| `direct_declaration_export_proof` | Source-header producer and validator share one unique-export/owner classifier and hash | REQUIRED | Pure function; retains nothing | `verter_session/src/resolver_store.rs` | always |
| `RootCapture::source_header_reader`, `StoreViewRoots::source_header_reader` | Supplies source-header payload for content already established by the captured root or completion overlay | REQUIRED | Opaque point-read capability wrapping a private weak scheduler handle; neither exposes enumeration, prolongs scheduler execution nor retains a source snapshot | `verter_session/src/store_view_roots.rs` | always |
| `direct_declaration_export_hash_for_content`, `check_direct_declaration_export_for_content`, source-header arms of `check_parse_domain` | Exact-content header validation; missing source, tombstone or content mismatch fails closed | REQUIRED | The source snapshot is held only during the read; no result store | `verter_session/src/store_view_roots.rs`, `resolver_store.rs`, `resolver_core/request_store_view.rs` | always |

The source-header hash contains the selected name, exact owner, type/value
capabilities and export type-only flag. Spans select the unique containing
declaration but offsets and declaration body hashes do not enter the hash.
The weak reader supplies payload only after matching the view-established
whole hash; it never replaces captured membership/currentness authority.
Source-header facts use the existing syntactic-route audit category; no new
optional counter or trace is added.

## Content reads and selected member ingress

| item | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ClassMemberInput`, `OwnedLowering::class_member_input` | Selected class-member lowering before broad declaration preparation | REQUIRED | Transient owned typed-IR ingress and exact resolved import identities; never a shallow input, query key or retained body cache | `source/deref.rs`, session `owned_lowering_port.rs` | always |
| `precise_class_member_read` | Shared lowering of closed property and annotated monomorphic method/accessor signatures; imported heads are supported for properties | REQUIRED | Graph result and its ordinary dependency evidence; unsupported scopes/shapes keep whole-content reads | type engine `project_semantic_dispatch/build.rs` | always |
| Header binder framing in `class_member_read_hash_in` | Member observation production and current-fact validation | REQUIRED | Temporary signature framing passed to the existing semantic hasher; no evaluator or store | semantic source `decl_body_memo.rs` | always |
| Content-free `fingerprint_of` population fields | Global name membership, owner and precedence validation | REQUIRED | Existing immutable contributor snapshot; consumed content stays with exact read ports | query `inputs/contributors.rs`, session `global_contributors/mod.rs` | always |
| `IndexedInputs::source_is_available` | Carrier file-availability probe | REQUIRED | Serves only availability; records the exact served `SyntacticRouteInterface`, preserves served refusal; default delegates the broad port | engine `request_ports.rs`, session `request_bound.rs` | always |
| `prepare_validation_inputs` | Request-bound fact/signature validation | REQUIRED | Produces current canonical parse facts and completes the existing request overlay before its validation epoch is captured; target populations additionally require the exact current published lease to agree with captured proof; no new store | session `request_bound.rs` | always |
| Whole-content observations at broad indexed/shallow lease serves and reacquisition, generic body/declaration/structure ports | Content consumers and their enclosing tracers | REQUIRED | Existing retained source identity supplies the observed hash; no current-hash re-read | session `request_inputs.rs`, `owned_lowering_port.rs` | always |
| `PreparedDeclBundle::resolution_facts`, `seal_resolution_facts`, `observe_read` | Generic prepared declaration and scope consumers, including memo/shared/follower hits | REQUIRED | Write-once, self-charged owned witness rail, sealed before publication and released with the bundle; missing import proof refuses retention | session `prepared_decl.rs`, `host_manage/prepared_decl.rs` | always |
| `IndexedExpressionDemand::source_identity`, `HostExpressionDemand::observe_content` | Every use of a retained expression/flow demand | REQUIRED | Borrows the already retained source key; adds no copied field, cache or counter | semantic source `decl_body_memo.rs`, session `host_source_demand.rs` | always |
| `CompletedSource::source_version` in the existing completion hash map | Source-header validation after a cold load | REQUIRED | Scalar admitted incarnation/generation paired atomically with its hash in the existing request owner; artifact-only completions fail header proof closed | session `request_store_view.rs` | always |
| Source-header `note_owner_visit` call | Root-capture complexity control | OPTIONAL | Existing test observation scope; no retained per-operation state | session `store_view_roots.rs` | `test` or default-off `semantic-observe` |
| `AugmenterEntry::population_hash`, `augmentation_population_hash` | Target-wide binding membership and exact-owner validation | REQUIRED | Inline digest in the existing immutable population; excludes body, member-header and source-content versions | query `resolution/augmentation_key.rs`, `source/augmentation.rs` | always |
| Exact served augmentation artifact port | Augmentation content and negative named-contribution reads | REQUIRED | Existing current artifact key and retained payload; whole-content observation precedes delivery; no canonical-wide scan | session `owned_lowering_port.rs`, `file_artifact_store.rs` | always |
| `AnalysisSourceRevision::whole_hash`, analysis/structure content observation | Standalone snapshot consumers | REQUIRED | Existing exact snapshot revision, or a hash of the actually retained registered structure bytes; unstamped revisions refuse retention | query `analysis/file_analysis.rs`, session read ports | always |
| Population-only candidate loads and carrier identity hash reads | Augmentation discovery and lazy reference identity | REQUIRED | Existing overlay-aware availability/identity services; no broad content payload is exposed. A precise member read that embeds a referenced target's identity observes that target's exact current `FileWholeHash` — the identity is a reference-only consumer's sole contact with the target — while payload-reading consumers keep their own content proof | engine `build.rs`, `carrier.rs` | always |

| Authority-first augmentation candidate selection | Target population discovery | REQUIRED | Warm sets reacquire only their known contributors; cold overlay/base key selection precedes matching; exact source owner rejects unavailable or fenced serves; unrelated noncontributors are skipped | session `file_artifact_store.rs`, `source_augmentation.rs` | always |
| Neutral empty-interface suffix guard | Augmentation fold graph-work cutoff | REQUIRED | Recorded nongeneric class header and exact retained empty interface body, population and source environment; no cached body or new mode | engine `build.rs` | always |
| Empty versus nonempty suffix graph-work control | Regression tests | OPTIONAL | Existing graph stats under captured execution; no new production counter | session `class_member_evidence_tests.rs` | default-off `semantic-observe` |

| `Walker::lane`, `compute_display_hash`, `class_member_read_hashes_in` and Member Display observation | Source-independent class-member graph payloads | REQUIRED | One bounded shared grammar and retained selection, with existing FactLane determining cosmetic-name hashing; no new query mode/store; both lanes refuse on budget exhaustion | query `facts/hashing.rs`, semantic source `decl_body_memo.rs`, session `fact_emission.rs`, `owned_lowering_port.rs` | always |
| Old-or-new contributor queue predicate | Lazy canonical population retraction | REQUIRED | Reads exact retained previous snapshot and committed new source through existing classifiers; pending source identity lives in the existing ingestion queue | session `host_upsert.rs`, `host_construction.rs` | always |

| Cold population `scan_root`, installer `scan_state`, `populate_augmenter_set_at_root` | Atomic publication against candidate-corpus changes | REQUIRED | Existing fully-applied artifact-root lease lives for one scan; existing registry mutex covers own-only epoch admission through insertion and releases before epoch completion; callbacks hold no storage guards; no new store/counter | session `source_augmentation.rs`, `file_artifact_store.rs` | always |

Required proof hooks are unconditional. Work controls use existing counters and
captured execution; only the source-header complexity observation above is a new
optional capture hook. No timer, runtime enable flag or content-version query
key is introduced.

Source-header payload reads compare hash, incarnation and generation against
captured or explicitly completed source identity. Same-byte node replacements
cannot lend a new parsed payload to an old view. A session override cannot use
the base parser payload, even if its bytes match.

## Retained reference-version observation bounds

| item | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `NodeStructureBits::versioned_ref`, `node_reaches_versioned_reference` | Bounds the retained-reference version observation at every cold-build close: a value whose retained closure reaches no versioned `DeclRef`/`InstantiationRef` costs one memo read, and the walk descends only ref-reaching structure | REQUIRED | Pure inductive per-node bit in the existing append-only arena sidecar; decided once per node id, dropped with the node | engine `semantic_query_memo/unresolved_reach.rs`, `project_semantic_dispatch/arena_ops.rs` | always |
| `FactReadSet::holds`, `tracer_holds_fact` | Skips re-recording an embedded version fact the active tracer already holds through a delivered or absorbed witness; the sealed signature keeps the same union | REQUIRED | Read-only query over the live tracer's observations and absorbed runs; no stored state | query `facts/fact_read_set.rs`, engine `fact_tracing/tracing.rs`, `resolver_core/resolver_context.rs` | always |
| `FactReadSet::run_addresses` | Recognises a replayed canonical signature the scope already absorbed by storage address, so a warm read's repeated replays grow neither the run list nor comparison work | REQUIRED | Per-tracer index mirroring `canonical_runs`; inserted and removed with its runs and cleared at canonicalisation | query `facts/fact_read_set.rs` | always |
| Nested ResolveCall root member version observation at queue time | The drain publishes the member under the enclosing root carrier's read set | REQUIRED | Same observation as the batch members in `relation.rs`, performed before the member leaves the frame | engine `project_semantic_dispatch/call_resolve.rs` | always |
| `PreparedDeclBundle` resolution-segment retention charge | Shared-cache admission and bundle readers see the segments' vector, specifier and narrow fact-slice bytes | REQUIRED | Charged with the bundle's own retention via the existing heap meter; wide tree-sealed sets keep their own charge | session `resolver_core/prepared_decl.rs` | always |
| `subset_memo` (`FactSet::storage_weak`) | Canonicalisation skips a non-base run an exact membership scan or unchanged union construction already proved contained in the base run it seals against, so a replayed run a held seal covers is not re-merged; the sealed signature keeps the same union | REQUIRED | Bounded per-thread proof map from run-storage address pair to weak handles of both allocations; entries die with their storages, prune at 4096 entries and reset at 8192 | query `facts/fact_set.rs`, `facts/fact_read_set.rs` | always |

The versioned-reference bit and the address index add no optional counter,
trace or enable flag; they only bound the required observations' cost. Every
embedded version observation remains unconditional at the close that retains
a reference the scope does not already hold.

Canonicalisation uses transient borrowed hash sets for wide base membership and
distinct additions. Narrow deltas probe the persistent base directly rather
than rebuilding its index at every enclosing seal. The choice compares the
remaining probe count times logarithmic search depth with the base width,
after excluding inputs covered by existing subset proofs; a probe count
reduced by a remembered construction delta counts only the facts it still
probes. The filtered input list and both hash sets are REQUIRED merge
workspace living only during sealing; it owns no fact storage, cache identity
or optional observation. Exact equality preserves conflicting versions. Small
deltas extend the shared base tree; additions wider than the base are sorted
and merged into a bottom-up set.
The additions vector retains first-seen input order, preserving ordered runs
for sorting instead of scrambling them through hash-index iteration.
Signature work charges base visits, input visits, sorting and merge visits.
Known subset inputs need no repeated base walk or probe.
An unchanged union records each input as a subset using the existing weak
storage proof memo. Receipt or domain compaction disables this inference,
since the resulting set may have replaced an input fact.
Two per-demand proof memos bound the repeated cost further, both keyed by
storage address with weak handles validating liveness, both bounded by
prune/reset caps like the subset memo, and both drained — and closed to new
entries — the moment this thread's last tracer pops, so a finished demand
pins no fact storage: the seal-delta memo remembers each seal's construction
(the widest base it extended and the exact additions over it), so a later
seal scanning that run against a base provably holding its construction base
probes only the remembered additions; the base-index memo remembers the
membership index a seal built for its widest base, so sibling seals extending
the same shared run never re-walk it. A prepared-declaration read that uses
several authored imports replays the union of exactly those segments as ONE
combined run, cached per used-import set on the bundle, bounded, and charged
with the bundle's own retention, so an enclosing seal merges one run per used
set instead of one run per import.
This prevents repeated tree-depth lookups for overlapping evidence, growing
tree insertion for wide disjoint batches, and the once-per-enclosing-level
re-merge of a nested scope's whole witness; existing growth checks detect
those regressions but cannot perform the merge. No proof or refusal is
removed.
