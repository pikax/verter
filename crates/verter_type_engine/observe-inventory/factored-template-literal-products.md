# Factored template-literal product inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `TemplatePlan` (`texts`, per-span normalized `TemplateConstituent`s) — replaces the eager per-span prefix-product buffers | The template reducer weighs, charges and streams a template literal type's product from it; a nested template in a span builds its constituents from its own plan | REQUIRED | One template reduction | `src/project_semantic_dispatch/template_product.rs` | always |
| `TemplatePlan::cardinality` (checked product, an empty span wins wherever it sits) | The checker's cross-product limit (TS2590), the `never` answer of an empty product, and the work units charged before construction | REQUIRED-budget | Computed on demand from the plan | `src/project_semantic_dispatch/template_product.rs` | always |
| `TemplatePlan::construction_bytes` (factored literal bytes, holes, the union the concatenations join and the two copies of each member's text canonicalizing it holds) | The construction bytes reserved on the connected-work ledger before the first concatenation is built | REQUIRED-budget | Computed on demand from the plan | `src/project_semantic_dispatch/template_product.rs` | always |
| `connected_demand::UNION_ARM_BYTES` | The per-arm working set a constructed union holds while canonicalized, charged by the template reducer for the union its concatenations form | REQUIRED-budget | Constant | `src/project_semantic_dispatch/connected_demand.rs` | always |
| `TemplateCursor` (one choice per span) | Builds one concatenation at a time, each text at its exact length, in product order | REQUIRED | One read loop | `src/project_semantic_dispatch/template_product.rs` | always |

## verter_bench

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `Observation::checker_recovery` (diagnostic code of a recovered answer), filled by `NodeRoot::checker_recovery_code` | The benchmark's answer validation: a TS2590 recovery is told apart from a type the engine built, whose wire form is the same `any` | REQUIRED-validation | One observation record | `crates/verter_bench/src/semantic_perf/mod.rs` | always |

No optional counter, trace or attribution history is added, so nothing is gated behind `semantic-observe`. The prefix-product concatenation buffers, their per-step byte reservations and the per-piece byte helpers they used are deleted.
