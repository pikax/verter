# Relation evidence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RelationPayload` outcome, bindings and recursion footprint | Relation consumers and the warm-replay gate | REQUIRED | The `Relate` family candidate; evicted by the family rails or a document close | `src/semantic_query.rs` | always |
| Checker transaction assumption / discharge state (`RelationAssumptionEvidence`, SCC ledgers, `RecursionOrBudgetCap` poison) | Coinductive discharge, session admission and the public `BudgetExceeded` outcome | REQUIRED | One dispatch transaction | `src/project_semantic_dispatch/{dispatch_txn,relation}.rs` | always |
| `RelationExplanation` capture buffer and its producers at the cold decision sites | none; diagnostics and measurement readers via `take_relation_explanations` | OPTIONAL | One request: filled by cold relation decisions, drained by the reader or dropped with the dispatch; each `LeasedOperand` leases its operand payload until the explanation drops | `src/project_semantic_dispatch/relation_explanation.rs` | `cfg(feature = "semantic-observe")` |
