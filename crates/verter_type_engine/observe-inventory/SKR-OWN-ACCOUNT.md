# Semantic allocation ownership and charge transfer inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Every row below is a REQUIRED policy charge or a REQUIRED current count:
nothing here records allocation or attribution history, and nothing is
gated on `semantic-observe`. Policy reservations use the one process-local
retention account; no computation-memory cap is added — graph storage and
flights are `Pinned` (charged, never refused), and a refused cache
admission returns its complete value uncached.

## Declared conventions

| Convention | Rule | Owner |
| --- | --- | --- |
| Heap block | `n > 0` payload bytes cost `n` rounded up to 16 plus a 16-byte header; zero bytes allocate nothing (`heap_block_bytes`) | `verter_session_query/src/retention/allocation.rs` |
| Capacity | `String` / `Vec` cost their capacity, never their length | same |
| `Arc` block | an `Arc<T>` / `Arc<[T]>` / `Arc<str>` block holds its two counters beside the value (`arc_block_bytes`); an empty slice still costs its counters | same |
| Embedding | a field's inline bytes ride the block of the value embedding it; a value held directly in a container slot rides the container's slot convention | same |
| Container slot | one map / FIFO slot costs `ENTRY_OVERHEAD_BYTES` (fixed-size slot, so a fixed figure); variable records an entry keeps are charged on top | `retention/mod.rs` |
| Self-charged allocation | an evidence page, a cost receipt, a pooled identity string and a graph node carry their own charge; a holder pays only the handle's inline bytes | `allocation.rs` |
| Aliases within one charge | each distinct `Arc` allocation counts once per charge (`AliasSet`) | `allocation.rs` |
| Aliases across owners | an allocation independently charged owners share is charged once through `AliasCharges`, keyed by address; the charge co-owns the allocation | `retention/shared.rs` |
| Leaf text | a leaf `Arc<str>` name inside a charged carrier is counted in full with the carrier batch holding it (it has no charge of its own to share) | `allocation.rs` |
| Granular storage pin | graph storage is charged in whole granules: never below the live total, above it by less than two granules, released entirely when the storage is | `retention/granular.rs` |
| Publication slot bound | a memo candidate pays for every slot its publication may populate (the slot plus each sibling a recorded point satisfies) | `semantic_query_memo/family.rs` |

## Allocation rows

| Production owner | Allocations | Byte / capacity computation | Sharing | Transfer / release | Proof |
| --- | --- | --- | --- | --- | --- |
| Memo candidate — own (`MemoRetention::_own`) | inline `MemoEntry` per populated slot, slot convention, materialised-point slice and paths, delivery-lease owner and root sets, reverse-index records and the candidate's `Arc<FamilyKey>`, counters of empty shared slices, newly claimed evidence pages | `own_retained_footprint_bytes` + `registration_bytes` (records × slots, ⅞ table slack) | one per candidate; backfill clones and reader snapshots share the `Arc<MemoRetention>` | released when the candidate drains — never handed to a caller | `a_read_outliving_its_candidate_keeps_exactly_the_delivered_rail_charged` |
| Memo candidate — carrier (`_carrier`) | fact slice with every fact's owned canonical text and leaf names; self-root list and texts | `fact_slice_bytes`, `arc_str_list_bytes` + `registration_bytes` | one registered charge per allocation, shared by the root, its prefix backfills and SCC members | the last candidate holding it releases it; never delivered | `an_scc_member_shares_the_carrier_charge_and_keeps_it_after_the_root_drains` |
| Memo candidate — delivered (`delivered`) | walker-diagnostic slice with names and nested errors; interned dependency rail with canonicals; the value's outermost shared allocation and its heap | `OwnedHeap` over `ShallowDiagnostic` / `QueryError` / `DepSignature` / `SemanticQueryValue` + `registration_bytes` | registered once per allocation; caller holds ride every delivery lease of the candidate (warm reads move the entry's lease; the cold winner's lease gets the holds for the allocations it aliases) | last cache hold → `Pinned` for surviving readers, released with the last reader | `a_read_outliving_…`, `reads_dropped_first_leave_one_retained_charge_until_the_candidate_drains` |
| Refusal summary (`RefusalSummary::retention`) | summary block and table / order slot, read's lease metadata, request prefix paid lists and identities; carrier and delivered allocations as above | `own_retained_bytes` + shared holds | delivered allocations shared with the read and any memo candidate aliasing them | summary drop releases its own part; the read keeps its rail pinned | `a_sealed_refusal_hands_its_delivered_rail_to_a_surviving_read`, `retained_request_identities_grow_the_reservation` |
| Component-meta result entry and retained refusal | payload (record-family convention, below), fact rail with owned text, slot convention, request prefix | `ComponentMetaResultEntry::retained_footprint_bytes`, `RequestSpent::owned_heap_bytes` | the entry's charge rides the shared entry | released with the last entry holder | existing component-meta retention suite |
| Deferred-evaluation memo entry | entry block, slot and FIFO record, delivery lease, proof facts with owned text, self-root list | `fact_slice_bytes` + `arc_str_list_bytes` + lease metadata | one per entry | released with the entry | `evaluate_deferred_memo` suite |
| Substitution memo entry (`SubstituteMemoEntry`) | key / value slot, root owner and operand list | `hash_cons_entry_bytes` | one per entry | released with the entry; refused → recomputed on demand | `a_substitution_answer_is_charged_while_its_memo_keeps_it` |
| Memo family container (`FamilySlots::_retention`) | family key and slot lists in the map, control byte, budget-ledger record | `family_container_bytes` | one per keyed family | released when the family leaves the map; an SCC batch prepays every family it newly keys | family memo suites |
| Cost receipt (`DemandCostReceipt::retention`) | receipt block, prerequisite slice backing, own identity key block and encoded bytes, root attachment | `retained_bytes` | in-allocation `OnceLock` charge, reserved by the first store retaining a result it costs | released with the receipt's last holder | receipt retention suite |
| Request prefix (`RequestSpent`) | both paid lists' backings and every identity's key block and roots | `owned_heap_bytes` | counted with the summary retaining it (a paid identity can outlive its receipt) | released with the summary | `retained_request_identities_grow_the_reservation` |
| Identity pool (`IdentityInterner`) | each pooled `Arc<str>` block and map slot | `pooled_entry_bytes` | pooled allocation shared with callers | pool-only entries shed under pressure; held entries stay | `pooled_payload_is_charged_to_the_shared_account`, `the_project_store_charges_its_interned_identities` |
| Node arena (`ArenaInner::storage`) | live chunks (box, four slot arrays, table entry), live payload `Arc` blocks | `chunk_storage_bytes`, `payload_block_bytes`, granular pin | per arena | released as slots and chunks are freed; the store's teardown releases the rest | `graph_storage_is_pinned_while_its_nodes_live_and_released_with_them` |
| Arena dedup shards (`ShardIndex::storage`) | each shard's table capacity and spilled buckets | capacity × (entry + control byte) + spilled bucket blocks | per shard | follows the table; retained capacity stays charged while the table holds it | same |
| Signature epoch (`EpochInner::storage`) | every table's boxcar buckets, record heap (`RecordHeap`: spellings, layouts, binder spaces, sets, sequences, substitutions), dedup indexes, side maps (`graph_roots` incl. root owners, `descriptor_sources`, `space_key_owners`) | bucket opened at each power-of-two position, record heap, capacity deltas | one per epoch, shared by its tables | released when the epoch's last holder drops (retirement) | `a_retired_signature_epoch_releases_its_storage_with_its_last_holder` |
| Flight cell (`FlightCell::_storage`) | cell block and flight-table slot | `flight_storage_bytes` | in-allocation pin | released with the last subscriber holding the cell | session memo suites |
| Union views, binder dependencies, structural domains | view slices / dependency sets / key domains | existing capacity-based figures | one per entry | released with the entry | existing union-view / hash-cons suites |

## Fields, stores and hooks

| Fields, stores and hooks | Consumer | Classification | Lifetime / owner | Gate |
| --- | --- | --- | --- | --- |
| `allocation::{heap_block_bytes, arc_block_bytes, string_bytes, vec_bytes, box_bytes, AliasSet, fact_slice_bytes, fact_vec_bytes, fact_heap_bytes}`, `ResolutionFactKey::owned_heap_bytes` | Every charge's byte computation | REQUIRED | Computed on demand; the alias set lives for one computation (`verter_session_query/src/retention/allocation.rs`, `facts/resolution.rs`) | always |
| `RetentionCharge::{transfer_to_pinned, try_adopt_retained, absorb}`, account `reclassify` / `adopt_pinned` | Cache → caller transfer, re-adoption, granular growth | REQUIRED | Under the account's linearization point (`retention/mod.rs`, `granular.rs`) | always |
| `CacheHold`, `CallerHold`, `AliasCharges` registry, `registration_bytes` | Shared-allocation charges | REQUIRED | One registration per allocation still held; removed by the charge's own teardown, so the registry is bounded by live allocations (`retention/shared.rs`) | always |
| `GranularPin` | Graph storage charges | REQUIRED | Owner's storage lifetime (`retention/granular.rs`) | always |
| `OwnedHeap` and its impls, `ObjectProjectionFormula` / `SignatureSetValue` impls, `RecursiveRefArgs::operands_arc`, `BroadRuntimeClassification::kinds_arc`, `ObjectProjectionFormula::alternatives_arc`, `NodeRootSet::{owned_heap_bytes, heap_bytes}`, `SemanticDeliveryLease::owned_heap_bytes`, `CostIdentity::owned_heap_bytes`, `RequestSpent::owned_heap_bytes`, `CanonicalTypeSubstitution::retained_heap_bytes` | Engine carrier footprints | REQUIRED | Computed on demand (`semantic_query/allocation_footprint.rs` and owning modules) | always |
| `MemoEntry::retention` (`MemoRetention`, `SharedParts`, `SharedHolds`), `SemanticGraphStore::alias_charges`, `SemanticDeliveryLease::attach_retention` (`DeliveryOwner::retention`) | Memo candidate and refusal-summary charges and their delivery | REQUIRED | Candidate / delivery lifetime (`semantic_query_memo/{family,retention,delivery_lease,refusal_summary,scc_publish}.rs`) | always |
| `FamilySlots::_retention`, `family_slots_locked` / `family_slots_prepaid_locked`, `StagedMember::family_charge` | Family container charge | REQUIRED | Family keying lifetime (`family.rs`, `retention.rs`, `scc_publish.rs`) | always |
| `SubstituteMemoEntry` | Substitution memo charge | REQUIRED | Entry lifetime (`hash_cons_memos.rs`) | always |
| `ArenaInner::storage`, `ShardIndex::{storage, table_bytes}`, `NodeArenaCore::retention` | Arena storage charge | REQUIRED | Arena lifetime; account bound by the one store constructor `SemanticGraphStore::with_arena` before the first intern (`arena.rs`, `mod.rs`) | always |
| `EpochStorage`, `RecordHeap`, `AppendInterner::storage`, `EpochInner::storage`, `SignatureStore::account` | Signature epoch storage charge | REQUIRED | Epoch lifetime (`signature_kernel/{storage,lifetime}.rs`) | always |
| `FlightCell::_storage` | Flight storage charge | REQUIRED | Cell lifetime (`inflight.rs`, `producer.rs`) | always |
| `SemanticGraphStore::graph_storage_charge_bytes`, `HostRetentionSnapshot::semantic_graph_storage_bytes` | Current graph-storage pin in the host's retention snapshot | REQUIRED-lifetime | Derived on demand; no stored history (`retention.rs`, session `host_lifecycle.rs` / `types.rs`) | always |
| `shared_allocation_registrations_for_tests`, `graph_storage_charge_for_tests`, `slot_storage_charge_bytes`, `MemoRetention::{own_bytes, shared_holds}`, `FlightCell::new`, `AppendInterner::new` | Fixtures and independent accounting expectations | REQUIRED-test | Fixture lifetime | `test` or `test-support` |

## Open owners

These owners are not yet charged by their actual allocations; each is an
unaddressed item of this task, not optimization debt:

- component-meta payload records (`footprint::{SURFACE,STRUCTURAL,TYPE}_RECORD_BYTES`
  in `CachedComponentMetaResult::retained_footprint_bytes`) are still a
  per-record convention over a variable typed payload;
- a family key's own heap (instantiation arguments, interned identity
  handles) beyond its inline bytes;
- a graph payload's owned heap beyond its `Arc` block (member arrays and
  text shared between payloads through `PayloadArray::share`);
- the weak identity indexes and the dependency-signature interner's weak
  table (bounded and reported through `IdentityIndexSnapshot`, not charged);
- result-evidence receipts (`ResultEvidence` of a completed result — pages
  are charged, result evidence is not).
