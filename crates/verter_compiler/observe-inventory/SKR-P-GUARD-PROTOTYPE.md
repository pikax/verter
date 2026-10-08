# Flow-transparent callback check inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Every item below lives in `src/ide/template/flow_check/`, which compiles only under
`cfg(any(test, feature = "test-support"))`; no production build contains it.

## verter_compiler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CheckPlan` and its items (`Chain`, `Branch`, `Frame`, `Callback`, `ExprItem`, `OuterRef`, `ScopeKey`, `BranchKey`): resolved conditions, predecessor links, scope keys and callback inputs | none; the generator and its tests | REQUIRED (generated semantics) | Built per template by the caller; dropped after generation | `src/ide/template/flow_check/seam.rs` | `cfg(any(test, feature = "test-support"))` |
| `GeneratedCheck::code` and `GeneratedCheck::mappings`: generated check and its authored provenance | none; the compiler and paired real-provider tests | REQUIRED (generated semantics and mappings) | Owned by the returned value | `src/ide/template/flow_check/seam.rs` | `cfg(any(test, feature = "test-support"))` |
| `GenerationWork` counters (conditions, predecessor links, scope links, callbacks, outer references, transform operations, replayed terms) | none; growth tests only | OPTIONAL | Per `generate` call, returned by value | `src/ide/template/flow_check/generator.rs` | `cfg(any(test, feature = "test-support"))` |
| `Layout` byte accounting and `CallbackSite` list | none; growth and coverage tests only | OPTIONAL | Per `generate` call, returned by value | `src/ide/template/flow_check/generator.rs` | `cfg(any(test, feature = "test-support"))` |
| `BuildWork` counters (conditions resolved, scope handles read, callbacks planned) | none; contract tests only | OPTIONAL | Per `build_plan` call, returned by value | `src/ide/template/flow_check/builder.rs` | `cfg(any(test, feature = "test-support"))` |

The counters are test-only measurement of a test-only module, so they carry no
`semantic-observe` gate and no per-operation enabled check. The production emitter
that later adopts the representation classifies its own state.
