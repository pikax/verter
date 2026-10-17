# Tuple-splice inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `checker_policy::tuple_splice` (with `TUPLE_SPLICE_SIZE` and `TupleSplicePosition`) | The tuple-product constructor refuses a splice whose expanded prefix and spliced tuple reach 10,000 elements with TS2799 (type) or TS2800 (expression), before an element is copied | REQUIRED | Stateless check | `src/semantic_query/checker_policy.rs` | always |
| `CheckerDiagnosticCode::ExpressionTupleTooLarge` | The TS2800 recovery an array literal's refused splice answers with | REQUIRED | Part of the recovery node | `src/semantic_query/checker_diagnostic.rs` | always |
| `SplicePlan` (`targets`, `len`, `read`) | The constructor sizes every spliced tuple, then charges `read + len` work units and `len` elements' bytes to the connected-demand ledger before it copies, and copies from the resolved targets | REQUIRED | One normalization | `src/project_semantic_dispatch/tuple_product.rs` | always |
| `ExpressionSplice` | `FlowEvaluator::array_eval_step` copies a spread tuple only on `Copy`; `Refused` is the literal's TS2800 recovery, `Unpaid` keeps the spread as a rest element | REQUIRED | One spread | `src/project_semantic_dispatch/tuple_product.rs` | always |

The splice charges go through the existing ledger rails (`charge_units`, `reserve_bytes`), so they accrue to the open cost scope and its receipt footprint like any other charge; no field is added to the ledger. No counter, trace or copy/allocation counter is added; the tests read the existing ledger usage, `SemanticGraphStats` misses and the refusal-summary count, so nothing is gated behind `semantic-observe`.
