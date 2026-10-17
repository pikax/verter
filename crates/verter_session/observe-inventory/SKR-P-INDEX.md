# Direct-edge receipt reverse indexes

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ReceiptReverseIndex` canonical table (canonical → direct holders) | `invalidate_canonical` of the family memo, the refusal table and the candidate store starts its walk here | REQUIRED-lifetime | A canonical lives while some registered evidence or candidate reads it directly; dropped with its last holder | `semantic_query_memo/reverse_index` | always |
| `ReceiptReverseIndex` evidence nodes (carriers, receipts, pages; cached or not) and their parent sets | The invalidation walk from a canonical up to the candidates reaching it; ownership of the evidence while indexed | REQUIRED-lifetime | Held while a registered candidate reaches the node; dropped with the last such candidate, outside the index lock | `semantic_query_memo/reverse_index` | always |
| `ReceiptReverseIndex` candidate nodes (key, payload, carrier, extra canonicals) | Unregistration on displacement, FIFO/budget eviction, epoch retirement, release and invalidation | REQUIRED-lifetime | Registered and unregistered in the same consistency step as the candidate's visibility; cleared with the store | `semantic_query_memo/reverse_index` | always |
| `IndexState.edges` and `ReverseIndexOccupancy` (`occupancy`, `SemanticGraphStore::reverse_index_occupancy`) | Current occupancy count of the index | REQUIRED-lifetime | Updated at each edge insert/remove under the index lock | `semantic_query_memo/reverse_index` | always |
| `IndexStorage` owned-byte total and `GranularPin` | Retention account charge for evidence-owned edges, evidence nodes and canonical text | REQUIRED-lifetime | Settled after each mutation; reaches zero when the index drains | `semantic_query_memo/reverse_index` | always |
| `ReceiptReverseIndex::candidate_bytes`, memo `registration_bytes` | Admission reservation of a candidate's own node and the edges naming it | REQUIRED-lifetime | Part of the candidate's reservation; released with the candidate | `semantic_query_memo/reverse_index`, `semantic_query_memo/family` | always |
| `MemoRegistration` (family, populated slots) | Family-memo eviction of exactly the reached `(family, slot, admission_seq)` candidates | REQUIRED | Payload of the memo registration; a slot leaves on its displacement, the registration with its last slot | `semantic_query_memo/reverse_index` | always |
| `RefusalSummaries.index` and `invalidate_refusal_summaries` | Explicit invalidation drops sealed refusals whose failed-prefix receipts reach the canonical; delivery still revalidates | REQUIRED-lifetime | Mutated under the refusal table lock with `entries`/`order`; cleared with the table | `semantic_query_memo/refusal_summary` | always |
| `ReverseIndexedCandidateStore.canonical_index` (now a `ReceiptReverseIndex`) | Candidate-store per-canonical invalidation | REQUIRED-lifetime | Registered under the slot guard and retention gate read guard; walked under the retention gate write guard | `cache_runtime/candidate_store` | always |
| `ReverseIndexEdgeOps` (`linked`, `unlinked`, `walked`) and `edge_ops` / `reverse_index_edge_ops` | none; edge-operation measurement | OPTIONAL | Index lifetime | `semantic_query_memo/reverse_index` | cfg(feature = "semantic-observe") |
| `record_family_map_lock_acquisition` at reverse-index registration and invalidation | none; request audit contention | OPTIONAL | Request audit | `semantic_query_memo/reverse_index`, `semantic_query_memo` | cfg(feature = "semantic-observe") |

Unit-test and test-support builds retain the edge-operation counters, which
behavioral registration-growth assertions read. Default production builds omit
them; their selection has no runtime enabled check.
