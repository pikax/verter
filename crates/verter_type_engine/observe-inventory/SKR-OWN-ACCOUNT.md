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
| Self-charged allocation | every result receipt and evidence page, a cost receipt, a pooled identity string, an interned identity record (intersection recipe, semantic context, order domain, relation / call key), a budget profile, a delivery owner and a graph node carry their own charge; a holder pays only the handle's inline bytes | `allocation.rs`, `facts/receipt.rs`, `intern_table.rs`, `cost_receipt.rs`, `delivery_lease.rs` |
| Aliases within one charge | each distinct `Arc` allocation counts once per charge (`AliasSet`) | `allocation.rs` |
| Aliases across owners | an allocation independently charged owners share is charged once through `AliasCharges`, keyed by address; the charge co-owns the allocation | `retention/shared.rs` |
| Leaf text | a leaf `Arc<str>` a registered carrier reaches is its own registered charge, held by every carrier reaching it in the carrier's class (cache or caller) and released with the last; registration measures with a collecting `AliasSet` | `allocation.rs`, `retention/shared.rs` |
| Caller-first charge | a delivery holds the allocations it hands out as the caller's from the moment it is minted — pinned, never refused; a cache retaining them adopts the same charge refusably | `retention/shared.rs` (`pin_caller`), `semantic_query_memo/delivery_lease.rs` |
| Arena shared heap | a graph payload's and scope's heap is charged with graph storage; every `Arc` it reaches is counted once arena-wide by owner count (payloads and enclosing shared allocations) | `semantic_query_memo/arena.rs`, `semantic_query/node_payload_footprint.rs` |
| Weak-held block | an interned dependency rail's block stays allocated while the interner's `Weak` lives, so the interner charges it for its entry's lifetime and a holder pays only the rail's names | `semantic_query_memo/interner.rs` |
| Process statics | the process-wide empty diagnostic list and dependency rail are static storage, never charged | `semantic_query_memo/mod.rs` |
| Granular storage pin | graph storage is charged in whole granules: never below the live total, above it by less than two granules, released entirely when the storage is | `retention/granular.rs` |
| Publication slot bound | a memo candidate pays for every slot its publication may populate (the slot plus each sibling a recorded point satisfies) | `semantic_query_memo/family.rs` |

## Allocation rows

| Production owner | Allocations | Byte / capacity computation | Sharing | Transfer / release | Proof |
| --- | --- | --- | --- | --- | --- |
| Memo candidate — own (`MemoRetention::_own`) | inline `MemoEntry` per populated slot, slot convention, materialised-point slice and paths, reverse-index record buckets and the candidate's `Arc<FamilyKey>` clone (block and the boxes the clone copies), newly claimed evidence pages | `own_retained_footprint_bytes(family, …)` + `reverse_index::registration_bytes` | one per candidate; backfill clones and reader snapshots share the `Arc<MemoRetention>` | released when the candidate drains — never handed to a caller | `a_read_outliving_its_candidate_keeps_exactly_the_delivered_rail_charged` |
| Memo candidate — carrier (`_carrier`) | fact slice with every fact's owned canonical text; self-root list; their leaf names as registered leaves | `fact_slice_bytes`, `arc_str_list_bytes` (collecting) + `registration_bytes` | one registered charge per allocation, shared by the root, its prefix backfills and SCC members; each leaf once across carriers | the last candidate holding it releases it | `an_scc_member_shares_the_carrier_charge_and_keeps_it_after_the_root_drains`, `a_name_a_diagnostic_and_a_dependency_rail_share_is_charged_once` |
| Memo candidate — delivered (`_delivered`) | walker-diagnostic slice and nested errors; dependency rail (an interned rail's names only — its block is the interner's); the value's outermost shared allocation and its heap; their leaf names | `OwnedHeap` over `ShallowDiagnostic` / `QueryError` / `SemanticQueryValue`, `dep_rail_heap_bytes` + `registration_bytes` | registered once per allocation; every delivery of the same allocation holds it as a caller from its mint | last cache hold → `Pinned` for surviving deliveries (leaves move with it), released with the last holder | `a_read_outliving_…`, `reads_dropped_first_leave_one_retained_charge_until_the_candidate_drains` |
| Delivery owner (`DeliveryOwner::_own`, `holds`) | owner block, both root owners and operand lists, hold list; caller holds on its diagnostics, dependency rail and value | `DeliveryOwner::charged`; `hold_delivered_allocations` (`pin_caller`) | the owner is shared by clones of its reads; held allocations share the registry charge; a cache retaining the owner (a memo candidate, a refusal summary, a deferred-evaluation entry) adopts its metadata charge refusably | metadata pinned to the owner's readers, retained while a cache holds it, released with the owner's last clone; caller holds with it | `a_refused_admission_returns_the_complete_value_charged_to_its_caller`, `a_read_outliving_…` |
| Refusal summary (`RefusalSummary::retention`) | summary block and table / order slot, request prefix paid lists and identities; carrier and delivered allocations as above | `own_retained_bytes` + shared holds | delivered allocations shared with the read and any memo candidate aliasing them | summary drop releases its own part; the read keeps its rail pinned | `a_sealed_refusal_hands_its_delivered_rail_to_a_surviving_read`, `retained_request_identities_grow_the_reservation` |
| Component-meta result entry and retained refusal | payload by its allocations (`component_meta_footprint`), fact rail with owned text, slot convention, request prefix | `CachedComponentMetaResult::retained_footprint_bytes`, `RequestSpent::owned_heap_bytes` | the entry's charge rides the shared entry | released with the last entry holder | component-meta retention suite |
| Deferred-evaluation memo entry | entry block, slot and FIFO record, proof facts with owned text, self-root list; its delivery owner charges itself | `fact_slice_bytes` + `arc_str_list_bytes` | one per entry | released with the entry | `evaluate_deferred_memo` suite |
| Substitution memo entry (`SubstituteMemoEntry`) | key / value slot, root owner and operand list | `hash_cons_entry_bytes` | one per entry | released with the entry; refused → recomputed on demand | `a_substitution_answer_is_charged_while_its_memo_keeps_it` |
| Memo family container (`FamilySlots::_retention`) | family key and slot lists in the map, control byte, budget-ledger record, the map key's heap and the ledger clone's boxes | `family_container_bytes(family)` over `family_footprint::family_key_heap_bytes` | one per keyed family | released when the family leaves the map; an SCC batch prepays every member's family before its lock, so keying under the lock never refuses | `family_footprint` suite, `an_scc_batch_never_publishes_without_a_member_whose_family_was_evicted_meanwhile` |
| Reverse index (`CanonicalToEntries::storage`) | each shard's copy of its canonical's text, each shard table's unused buckets, the outer table's buckets | settled per insert / prune / shard removal | per index | given back as shards drain; record buckets are the candidates' | `a_reverse_index_shard_charges_its_canonical_text_until_it_drains` |
| Shared-charge registry table (`RegistryTable::backing`) | table buckets by capacity | settled per registration | per registry | given back as registrations drain | `a_drained_registry_gives_back_its_table_and_its_charge` |
| Dependency-rail interner (`DepSignatureInterner::storage`) | shard tables, entry lists, every entry's weakly held block | settled per intern / prune / sweep | per store | released when an entry is pruned | `dep_signature_interner_charges_the_rail_blocks_it_keeps_until_swept` |
| Interned identity records and indexes (`InternRecord::_charge`, `IndexTable::backing`) | record block and value heap; digest table and spilled buckets | `InternDomain::value_heap_bytes`; settled per intern / forget | per record / per process-wide index | released with the record's last handle; backing given back as the index drains | `a_record_and_its_index_backing_are_charged_until_the_last_handle_drops`, `an_interned_relation_record_charges_itself_and_its_holders_pay_only_their_handles` |
| Budget profiles (`InternedProfile`, profile table) | profile block; profile table by capacity | pinned at intern; settled per intern / drop | per profile / process-wide | released with the profile's last holder; table shrinks as profiles drain | `churned_budget_profiles_are_charged_while_live_and_return_to_baseline` |
| Result receipts (`ResultEvidence::retention`) | evidence block, fact slice and owned heap, aggregated domains, canonical-set storage | `evidence_storage_bytes` | one per evidence, shared by every holder | pinned from birth, claimed into the first retaining admission, released with the last holder | `a_result_receipt_is_charged_once_for_its_variable_text_until_its_last_holder_drops` |
| Cost receipt (`DemandCostReceipt::retention`) | receipt block, prerequisite slice backing, own identity key block and encoded bytes, root attachment | `retained_bytes` | in-allocation `OnceLock` charge, reserved by the first store retaining a result it costs | released with the receipt's last holder | receipt retention suite |
| Request prefix (`RequestSpent`) | both paid lists' backings and every identity's key block and roots | `owned_heap_bytes` | counted with the summary retaining it (a paid identity can outlive its receipt) | released with the summary | `retained_request_identities_grow_the_reservation` |
| Identity pool (`IdentityInterner`) | each pooled `Arc<str>` block and map slot | `pooled_entry_bytes` | pooled allocation shared with callers | pool-only entries shed under pressure; held entries stay | `pooled_payload_is_charged_to_the_shared_account`, `the_project_store_charges_its_interned_identities` |
| Node arena (`ArenaInner::storage`, `shared_heap`) | live chunks (box, four slot arrays, table entry), live payload `Arc` blocks, every payload's and scope's heap, the shared-heap owner table | `chunk_storage_bytes`, `payload_block_bytes`, `charge_payload_heap` / `release_payload_heap`, granular pin | per arena; a shared allocation once arena-wide | released as slots and chunks are freed; the store's teardown releases the rest | `graph_storage_is_pinned_while_its_nodes_live_and_released_with_them`, `graph_storage_charges_payload_heap_once_per_shared_allocation` |
| Arena dedup shards (`ShardIndex::storage`) | each shard's table capacity and spilled buckets | capacity × (entry + control byte) + spilled bucket blocks | per shard | follows the table; retained capacity stays charged while the table holds it | same |
| Signature epoch (`EpochInner::storage`) | every table's boxcar buckets (boxcar 0.2.14 layout: first write of a bucket and the eager next bucket at ⅞), record heap (`RecordHeap`: spellings, layouts, binder spaces, sets, sequences, substitutions), dedup indexes, side maps (`graph_roots` incl. root owners, `descriptor_sources`, `space_key_owners`) | `opened_bucket_bytes` (per-table charged-bucket bits), record heap, capacity deltas; settlement re-reads the live total after publishing | one per epoch, shared by its tables | released when the epoch's last holder drops (retirement) | `a_retired_signature_epoch_releases_its_storage_with_its_last_holder`, `an_epoch_table_charges_every_boxcar_bucket_once_including_the_eager_next_one`, `a_shrinking_settlement_racing_lock_free_growth_still_covers_the_live_total` |
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
| `AliasSet::{collecting, take_leaves}`, `SharedState::leaves` (`LeafHolds`), `AliasCharges::{pin_caller, table_charged_bytes, table_capacity}`, `RegistryTable::backing` | Leaf charges, caller-first charges, registry backing | REQUIRED | Holder / registry lifetime (`retention/{allocation,shared}.rs`) | always |
| `DeliveryOwner::{holds, _own}`, `DeliveredAllocations`, `hold_delivered_allocations`, `with_value_allocation` | Caller charges minted with each delivery | REQUIRED | Delivery owner lifetime (`semantic_query_memo/{delivery_lease,retention}.rs`) | always |
| `ArenaInner::shared_heap`, `PayloadHeap` / `PayloadHeapVisitor` | Arena payload heap charge | REQUIRED | Live-slot lifetime (`arena.rs`, `semantic_query/node_payload_footprint.rs`) | always |
| `CanonicalToEntries::{storage, remove_shard, clear_shards}` | Reverse-index storage charge | REQUIRED | Index lifetime (`reverse_index.rs`) | always |
| `DepSignatureInterner::{storage, charged}`, `is_interned_rail`, `interned_rail_heap_bytes` | Interner storage charge | REQUIRED | Store lifetime (`interner.rs`) | always |
| `InternDomain::value_heap_bytes`, `InternRecord::_charge`, `IndexTable::backing`, `InternedProfile` charge, `ProfileInterner::backing` | Interned identity / profile charges | REQUIRED | Record and process-wide index lifetime (`intern_table.rs`, `cost_receipt.rs`) | always |
| `family_footprint::{family_key_heap_bytes, family_key_clone_heap_bytes, query_key_heap_bytes}`, `PreparedKeyHandle::producer_heap_bytes`, `result_footprint`, `flow_return_footprint`, `component_meta_footprint` | Key, result and payload footprints | REQUIRED | Computed on demand | always |
| `ResultReceipt::new_on`, `evidence_storage_bytes` | Result-receipt charge | REQUIRED | Evidence lifetime (`facts/receipt.rs`) | always |
| `EpochStorage::settle` re-read, `AppendInterner::charged_buckets` | Epoch charge coverage under concurrent growth | REQUIRED | Epoch lifetime (`signature_kernel/storage.rs`) | always |
| `store_storage_pins_for_tests`, `own_charge_bytes_for_tests`, `held_allocation_bytes_for_tests`, `record_charge_bytes`, `backing_charge_bytes`, `charge_bytes_for_tests`, `interner_backing_for_tests`, `storage_charge_bytes`, `set_settle_hook`, `ArenaRaceEvent::BeforeShardStorageSnapshot` | Fixtures and independent accounting expectations | REQUIRED-test | Fixture lifetime | `test` or `test-support` |
| `SemanticGraphStore::graph_storage_charge_bytes`, `HostRetentionSnapshot::semantic_graph_storage_bytes` | Current graph-storage pin in the host's retention snapshot | REQUIRED-lifetime | Derived on demand; no stored history (`retention.rs`, session `host_lifecycle.rs` / `types.rs`) | always |
| `shared_allocation_registrations_for_tests`, `graph_storage_charge_for_tests`, `slot_storage_charge_bytes`, `MemoRetention::{own_bytes, shared_holds}`, `FlightCell::new`, `AppendInterner::new` | Fixtures and independent accounting expectations | REQUIRED-test | Fixture lifetime | `test` or `test-support` |

## Open owners

These owners are not yet charged exactly; each is an unaddressed item of
this task, not optimization debt:

- a delivered allocation a caller clones out of a read (`CacheRead::dep_signature`
  returns the `Arc`, value carriers such as `SignatureSetValue` are `Clone`)
  outlives every charge holder once the read and the cache drop — pending the
  ownership-model ruling `skr-own-account-public-alias-lifetime`;
- deep public copies (`copy_in_scope` builder copies) are not charged at their
  copy sites — same ruling;
- a nested shared `Arc` slice other than a leaf `Arc<str>` (an error's alias
  chain, recursive-reference arguments) is counted with each carrier reaching
  it;
- a released graph payload a holder still keeps through an `Arc` is uncharged
  from its slot's release;
- inside a cached component-meta payload, a JSDoc tag's resolved type graph
  (`verter_protocol` `ResolvedTypeGraphSnapshot`, tables crate-private) and a
  publication's symbolic-equivalence proof (`verter_type_expr`
  `SymbolicEquivalenceProof`, no accessor) are charged their inline bytes
  only.
