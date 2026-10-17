# Settled request validation verdicts

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Rows cover the validation verdicts a request view settles and reuses, valid
or stale, and the receipt walk that proves which receipts a failure reaches.
No counter or trace is added: the leaf-read and settled-verdict counts the
tests read exist only in unit-test builds.

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ReceiptWalk::leaves_with_verdicts` and `ReceiptVerdict` | The request view's fact-signature validation: receipts it already settled answer without a walk, and a failing walk names the receipts proven to reach the failure, never a sibling it met without finishing | REQUIRED | One walk over one borrowed signature; the met-receipt list is dropped when the walk returns | `src/facts/receipt.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RequestStoreView.receipt_verdicts` (`ReceiptVerdicts`: receipt, strict roots it reaches and completion state, to valid or stale) | Every signature and single-fact validation through the request view: a receipt settled in the same context is not walked again, so a deep stale fact is read once per context | REQUIRED | One request view; recorded only when the completion state held across the validation and the failure was read from what the view captured, never on refusal, a moved or in-flight state, a read refused its declaration-body or walk-stack lease, or a live-store failure; dropped with the view, releasing every receipt it holds | `src/resolver_core/request_store_view.rs` | always |
| `RequestStoreView.signature_verdicts` (`SignatureVerdicts`: retained signature, strict roots and completion state, to valid or stale) | `FactValidation::validates_retained_signature_with_self_roots`, the warm-read validation of every retained cache candidate | REQUIRED | One request view; recorded under the same settlement rule; dropped with the view | `src/resolver_core/request_store_view.rs` | always |
| `RequestStoreView.leaf_validations`, `settled_receipts_for_tests`, `settled_signatures_for_tests` | none; unit-test assertions on reads and settled verdicts | OPTIONAL | One request view | `src/resolver_core/request_store_view.rs` | cfg(test) |
