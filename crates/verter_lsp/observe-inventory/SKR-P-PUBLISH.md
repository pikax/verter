# Incremental carrier-store publication inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
These rows cover the fields, stores and hooks of the incremental carrier-store
publication format: the commit pointer, the per-generation base and journal, the
writer's folded state, the readers' folds, and the deterministic work counters.
Durability and identity state is REQUIRED; the work counters are OPTIONAL and sit
behind the crate's default-off `semantic-observe` feature (test builds compile
them too), so default builds compile them away with no per-operation enabled
check. The commit-fault seams are `cfg(test)` only and absent from every shipped
build.

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `head.json` (`StoreHead::format`, `generation`, `instance`, `host_version`) | `CarrierPublishStore::sync`, `PublishedStoreReader::refresh`, the Node `DiskCarrierStoreReader.readFolded`: which generation is authoritative, and whether a folded cursor still follows it | REQUIRED | One per store dir; replaced atomically by store creation and by each compaction; `instance` is minted at creation and kept across compactions | `src/external_ts/carrier_publish_journal.rs`, `src/external_ts/carrier_publish_store.rs` | always |
| `snapshot-<generation>.json` (compacted base, a full `Manifest`) | Every cold load (writer reload, `read_published`, both followers on a generation change) | REQUIRED | Written before the head names its generation; the previous generation's base is retained across one compaction for readers that read the old head, older ones are deleted by the compacting writer | `src/external_ts/carrier_publish_store.rs` | always |
| `journal-<generation>.log` records (`JournalRecord::epoch`, `ops`, the FNV-1a line checksum) | Every commit (append) and every follower refresh (tail): the per-publication commit boundary and torn-write check | REQUIRED | Append-only within a generation; a torn tail is truncated by the next writer under `writer.lock`; retired with its generation | `src/external_ts/carrier_publish_journal.rs` | always |
| `writer.lock` (advisory exclusive file lock) | `CarrierPublishStore::commit`: serializes appends and compactions across LSP processes sharing one store | REQUIRED | Held only for the duration of one commit; released on drop or by the OS when the holder dies | `src/external_ts/carrier_publish_store.rs` | always |
| `WriterState::cursor` (`StoreCursor`: `generation`, `instance`, `journal_offset`, folded `StoreState`) | `CarrierPublishStore::commit`: reconciles each publication against the folded rows without re-reading published records | REQUIRED | One per store handle; created by the first commit, dropped after any failed commit (the next commit reloads from disk) | `src/external_ts/carrier_publish_store.rs` | always |
| `StoreCursor::journal_records` and `COMPACTION_FLOOR` | `CarrierPublishStore::commit` compaction decision: fold the journal once it holds `max(COMPACTION_FLOOR, live rows)` records | REQUIRED-budget | Reset to zero by each compaction and each reload | `src/external_ts/carrier_publish_journal.rs`, `src/external_ts/carrier_publish_store.rs` | always |
| `ProjectState::owned_order`, `provider_refs`, `owned_rows` (folded row indexes) | Publication reconciliation (`reconcile_publish_ops`, `retract_source_ops`) and the compaction measure | REQUIRED-lifetime | Maintained per applied op; live as long as the cursor that folds them | `src/external_ts/carrier_publish_journal.rs` | always |
| `WriterState::journal` (open append handle) | `CarrierPublishStore::append` | REQUIRED-lifetime | One handle for the current generation; dropped on compaction, reload or failure | `src/external_ts/carrier_publish_store.rs` | always |
| `WriterState::last_epoch` | `CarrierPublishStore::initialize`: seeds a store re-created after its directory vanished so the epoch never regresses within a process | REQUIRED | Process lifetime of the store handle | `src/external_ts/carrier_publish_store.rs` | always |
| `PublishedStoreReader::cursor` | `PublishedStoreReader::refresh` / `manifest`: the Rust incremental follower (strict test oracles and diagnostics) | REQUIRED-lifetime | One per follower; replaced on a generation or instance change | `src/external_ts/carrier_publish_store.rs` | always |
| `StoreWork` (`snapshot_loads`, `snapshot_rows_loaded`, `records_applied`, `journal_bytes_read`, `records_appended`, `compactions`, `compaction_rows_written`) and the `observe_work!` hook | none — measurement only; read by the publication-cost acceptance tests through `CarrierPublishStore::work` / `PublishedStoreReader::work` | OPTIONAL | Monotonic per store handle or follower; dropped with it | `src/external_ts/carrier_publish_journal.rs` | `cfg(feature = "semantic-observe")` (and test builds) |

## @verter/typescript-plugin (Node reader of the same format)

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DiskCarrierStoreReader.folded` (`FoldedStore`: `generation`, `instance`, `offset`, `epoch`, folded projects with `readyByCanonical` / `ownedByCanonical` indexes) | Every carrier host hook (`readyFile`, `ownedSourceFor`, `companionForSource`, `readyIdeSources`, …): one head read plus the records appended since the last hook | REQUIRED | One per plugin project reader; replaced on a generation or instance change, dropped when the head disappears | `packages/typescript-plugin/src/helpers/carrierStore.ts` | always |
| `DiskCarrierStoreReader.materialized` | `readManifest()` whole-manifest view | REQUIRED-lifetime | Built on demand, dropped by any applied record or reload | `packages/typescript-plugin/src/helpers/carrierStore.ts` | always |

Removed with this change: the whole-manifest `manifest.json` file, the writer's
`manifest_lock` and `last_epoch` advisory atomic, the Node reader's
`(mtimeMs, size)` change key and its `invalidateManifest` escape hatch, and its
per-snapshot canonical-index `WeakMap` caches.
