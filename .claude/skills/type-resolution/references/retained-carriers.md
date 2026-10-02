## Retired solver surface and retained carriers

The former arena solver kernel is deleted. `TypeQueryEngine`,
`TypeSolverHost`, `EvalEnvSolverHost`, `SessionSolverHost`, and their
solve/relation/project modules must not appear in production. Structural
guards in
`crates/verter_session/src/project_semantic_dispatch_invariants_tests.rs`
enforce that absence.

The historical `verter_semantic::analysis::type_solver` module name remains
only as a home for framework-neutral data consumed across crates:

| File | Retained data |
| --- | --- |
| `arena.rs` | query-local node and primitive/literal carrier types; no solver caches |
| `builtin.rs` | built-in utility classification and metadata |
| `host.rs` | root identity, origin, utility-source, and request-status DTOs |
| `prepared.rs` | prepared declaration facts and authored-body locators |
| `result.rs` | exactness, execution-status, and typed diagnostic carriers |

These types do not authorize evaluation. Component-meta and other consumers
resolve through `ProjectSemanticDispatch::execute(SemanticQueryKey::...)` and
project the resulting semantic graph only at their output boundary.

