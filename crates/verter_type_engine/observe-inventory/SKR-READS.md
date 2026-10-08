# Read-delivery inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DemandCostReceipt` (identity, exclusive `LogicalUsage` of work, bytes and request operations, prerequisite receipts, depth) | Every warm, joined or prefix read replays it through `ConnectedDemandLedger::replay_admit` before the result serves | REQUIRED-budget | Shared by the candidates, flights and consumer receipts that reference it; freed with the last | `src/project_semantic_dispatch/cost_receipt.rs` | always |
| `MemoEntry::cost_receipt` | The family-memo warm reads (`get_validated_value_impl`, the fast path) hand it to the reader; backfilled prefixes and SCC members share the publishing build's | REQUIRED-budget | The candidate | `src/semantic_query_memo/family.rs` | always |
| `PublishedMemoCandidate::cost_receipt` | The SCC member batch publishes its members under the root's receipt | REQUIRED-budget | One publication | `src/semantic_query_memo/mod.rs` | always |
| `InflightState::cost_receipt` | A subscriber replays the producer's receipt as a warm read does | REQUIRED-budget | One flight | `src/semantic_query_memo/inflight.rs` | always |
| `ReadCapture` receipt slot and `Served<T>` | The dispatch admits the delivered result's receipt (`admit_served`) or computes the result itself | REQUIRED | One read | `src/semantic_query_memo/producer.rs`, `src/semantic_query_memo/mod.rs` | always |
| `ClaimAttempt::recompute` | A claimant refused a stored result's receipt claims the key to compute it, leaving the candidate in place | REQUIRED | One logical claim | `src/semantic_query_memo/producer.rs` | always |
| Cost-scope stack with repeat flags, and the demand's paid set | Charges accrue to the computation recording on top; a repeat of a paid computation is recorded, never charged | REQUIRED-budget | One connected demand | `src/project_semantic_dispatch/connected_demand.rs` | always |
| `RequestBudget` operations-paid set | Replays charge a computation's request operations once per request | REQUIRED-budget | One request | `src/request_budget.rs` | always |
| `BudgetProfile` intern table | The refusal identity of an isolated root | REQUIRED-budget | Process; one entry per distinct allowance set | `src/project_semantic_dispatch/cost_receipt.rs` | always |
| `RefusalSummaries` (refusal read, observed facts and self-roots, failed-prefix charges, trip) | An exact isolated-root repeat answers from it after revalidating the facts | REQUIRED-budget (summaries); REQUIRED-lifetime (the bounded table) | Bounded FIFO of `REFUSAL_SUMMARY_CAP`; cleared with the memo | `src/semantic_query_memo/refusal_summary.rs` | always |
| `QueryDelivery::receipt` | The consuming frame records the delivered result's receipt as its prerequisite | REQUIRED | One delivery | `src/project_semantic_dispatch/query_frames.rs` | always |
| `ProjectSemanticDispatch::driven_root_traced` | The drive's entry seals its root's refusal on the facts the root frame traced | REQUIRED | One drive: set by the root frame's completion, taken by the entry | `src/project_semantic_dispatch/query_frames.rs` | always |
| `ProjectSemanticDispatch::published_results` (read, carrier, receipt, edit clocks) | A claim the memo cannot serve back to the dispatcher's view reads the result its own build published instead of recomputing it | REQUIRED-budget (the receipts); REQUIRED-lifetime (the table) | The dispatcher; an entry recorded before a workspace edit is never served | `src/project_semantic_dispatch/mod.rs` | always |
| Deferred-evaluation memo receipt (`evaluate_deferred_memo` value) | A hash-cons hit replays the evaluation's receipt before it serves | REQUIRED-budget | The memo entry (bounded FIFO, cleared with the memo) | `src/semantic_query_memo/hash_cons_memos.rs` | always |

No optional counter, trace or charge-attribution history is added: receipts, sealed failed prefixes and their validity rails are required budget state, so nothing is gated behind `semantic-observe`.
