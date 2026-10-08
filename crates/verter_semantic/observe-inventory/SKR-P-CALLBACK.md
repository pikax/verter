# Callback source identity inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FunctionProgramIndex::expressions_by_point`: indexed expression program point → its record's position (first record in source order wins a shared point) | `FunctionProgramIndex::expression`, read per call argument by call resolution and per program-expression source by flow-return evaluation | REQUIRED | Built once when `FunctionProgramIndex::from_discovery` seals a parsed file version; shared by every clone of the index (including `map_stable_hashes`) and released with it | `src/function_program.rs` | always |
| `PROGRAM_EXPRESSION_LOOKUP_VISITS` thread-local counter: expression point-key comparisons made by `FunctionProgramIndex::expression` (counted in the key's equality, so a scan grows with the file) | none; linear-lookup tests and measurement only | OPTIONAL | Per thread; accumulates until a reader resets it | `src/function_program.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CALLBACK_LINK_PROBES` thread-local counter read through `take_callback_link_probes`: callback position-key comparisons made while linking call arguments to callback positions (counted in the key's equality) | none; linear-linking tests and measurement only | OPTIONAL | Per thread; accumulates until `take_callback_link_probes` drains it | `src/analysis/function_program.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

## Transient operational state (not retained semantic stores or instrumentation)

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `link_callback_return_sources` callback position map (callback start → first callback in source order at that start) | Linking each function-valued call argument to its callback's return source; ordinary arguments and calls without a function-valued argument skip it | REQUIRED | One discovery build | `crates/verter_semantic/src/analysis/function_program.rs` | always |
