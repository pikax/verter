# Per-file class index inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ClassCollector` recorded classes, seen-span set, frame stack, file-scope depth and `const` class-initializer table | `ClassCollector::finish`, which resolves each class's base and hands the records to `FunctionProgramIndex::from_discovery` | REQUIRED | Owned by one function-program discovery build; consumed by `finish` and dropped when the build returns | `src/analysis/class_index.rs` | always |
| `HashVisitor::classes` hook: the hash fold records each class it walks into the build's `ClassCollector` | Class index population for every served body | REQUIRED | Borrowed for one function's hash fold | `src/analysis/function_program_hash.rs` | always |
| `TOP_LEVEL_CLASS_WALK_VISITS` thread-local counter: statements and expressions the walk outside served bodies visits | none; tests and measurement only | OPTIONAL | Per thread; never reset by production code | `src/analysis/class_index.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ClassIndex` records (class span, expression flag, heritage flag, directly declared member spans), resolved bases, enclosing-class links, member-span and class-span keyed maps | The protected-member rule's declaring-class and derivation reads (`member_owner`, `class_derives_from` in `verter_type_engine`) | REQUIRED | Built once per parsed file version when `FunctionProgramIndex::from_discovery` seals; retained and released with that `FunctionProgramIndex` | `src/function_program/class_index.rs` | always |
