# Tuple-splice inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `TUPLE_SPLICE_SIZE` and `splice_fits` | The tuple-product constructor's size gate refuses a splice whose expanded prefix and spliced tuple reach 10,000 elements, before an element is copied | REQUIRED | Stateless check | `src/project_semantic_dispatch/tuple_product.rs` | always |
| `checker_policy::tuple_splice_refusal` (with `TupleSplicePosition`) | The size gate's one bridge to mint the tuple-spread refusal, TS2799 or TS2800 by checking site | REQUIRED | Stateless | `src/semantic_query/checker_policy.rs` | always |
| `CheckerDiagnosticCode::ExpressionTupleTooLarge` | The TS2800 recovery a splice refused while an expression is checked answers with | REQUIRED | Part of the recovery node | `src/semantic_query/checker_diagnostic.rs` | always |
| `ExpressionCheckingSite` (the thread's `CHECKING_SITE`) | The flow evaluator holds it while it evaluates; `normalize_tuple_spread` reads it to choose TS2800 over TS2799 for a refusal | REQUIRED | One flow evaluation; restored when the guard drops | `src/project_semantic_dispatch/tuple_product.rs` | always |
| `SizingWork` (`read`, `charged`) and `SIZING_CHARGE_BATCH` | The sizing pass charges the elements it reads to the connected-demand ledger every 256 elements and on every exit, refusal included, so a trip or cancellation stops it and a refusal keeps the work that decided it | REQUIRED-budget | One normalization | `src/project_semantic_dispatch/tuple_product.rs` | always |
| `SplicePlan` (`targets`, `len`) | The constructor charges `len` work units and `len` elements' bytes before it copies, and copies from the resolved targets | REQUIRED-budget | One normalization | `src/project_semantic_dispatch/tuple_product.rs` | always |
| `ExpressionSplice` | `FlowEvaluator::array_eval_step` copies a spread tuple only on `Copy`, after its elements are charged and their bytes reserved; `Refused` is the literal's TS2800 recovery, `Unpaid` keeps the spread as a rest element | REQUIRED-budget | One spread | `src/project_semantic_dispatch/tuple_product.rs` | always |

The splice charges go through the existing ledger rails (`charge_units`, `reserve_bytes`), so they accrue to the open cost scope and its receipt footprint like any other charge; no field is added to the ledger. No counter, trace or copy/allocation counter is added; the tests read the existing ledger usage, `SemanticGraphStats` misses and the refusal-summary count, so nothing is gated behind `semantic-observe`.
