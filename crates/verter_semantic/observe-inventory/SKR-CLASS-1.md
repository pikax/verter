# Per-file class index inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ClassCollector` recorded classes, seen-span set, frame stack, file-scope depth and `const` class-initializer table | `ClassCollector::finish`, which resolves each class's base and hands the records to `FunctionProgramIndex::from_discovery` | REQUIRED | Owned by one function-program discovery build; consumed by `finish` and dropped when the build returns | `src/analysis/class_index.rs` | always |
| `HashVisitor::classes` hook: the hash fold records each class it walks into the build's `ClassCollector` | Class index population for every served body | REQUIRED | Borrowed for one function's hash fold | `src/analysis/function_program_hash.rs` | always |
| `DiscoveryWalk`: discovery's one walk of the syntax outside every served function, which discovers each top-level and namespace-member statement's served positions and records the classes it meets into the build's `ClassCollector` | Served-position discovery and class index population outside every served body | REQUIRED | Borrows the discovery build for its walk | `src/analysis/function_program_discovery_walk.rs` | always |
| `DiscoveryCtx::served`: the span of each served function and served field initializer, inserted as discovery serves it | `DiscoveryWalk`'s skip of the syntax the hash fold walks; the parameter-decorator class walk | REQUIRED | Owned by one discovery build | `src/analysis/function_program.rs` | always |
| Discovery statement-entry counter (`take_statement_entries_for_tests`) | none; tests only | OPTIONAL | Thread-local, reset on read | `src/analysis/function_program_discovery_walk.rs` | `cfg(test)` |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclLoweringCounters::class_elements_lowered`: class elements selective class-body lowering lowered | none; tests and measurement only | OPTIONAL | Per host; reset only by the provenance facade | `src/decl_lowering.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ClassIndex` records (class span, expression flag, heritage flag, directly declared member spans), resolved bases, enclosing-class links, member-span and class-span keyed maps | The protected-member rule's declaring-class and derivation reads (`member_owner`, `class_derives_from` in `verter_type_engine`) | REQUIRED | Built once per parsed file version when `FunctionProgramIndex::from_discovery` seals; retained and released with that `FunctionProgramIndex` | `src/function_program/class_index.rs` | always |

## Transient operational state (not retained semantic stores or instrumentation)

These fields are per-operation bookkeeping, not semantic stores, caches or counters; they are classified so the inventory covers every field this task adds or changes.

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `Lowerer::lowering_local_classes` span-start stack of the local class declarations whose value is lowering | `lower_local_class_declaration`'s circular-read guard (a class read inside its own lowering is unmodelled) | REQUIRED | Owned by one flow-slice lowering; pushed and popped around each local class lowering | `crates/verter_semantic_source/src/flow_slice_content.rs` | always |
| `ClassOwner::Syntactic.base` (`SyntacticBase`) the derivation chain of a syntactic class owner | `member_derives`' base-chain read | REQUIRED | An on-demand query value, built per relation read and dropped with it; never stored | `crates/verter_type_engine/src/project_semantic_dispatch/relation.rs` | always |
