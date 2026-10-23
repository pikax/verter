# Conditional continuation state

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The interface is described in [root continuations](../../../docs/arch/root-continuations.md).

| Item | Production consumer | Classification | Lifetime | Owner module | Gate |
| --- | --- | --- | --- | --- | --- |
| `ConditionalDecisionState.prepared`, `PreparedConditionalOperands` | The decision resumes selection from the operands already prepared; their adopted observations remain in the owning frame's taint and tracer | REQUIRED | One conditional frame, or one synchronous decision | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `ConditionalBuild.decision` | The query frame owns the decision's sequential position across dependency delivery and thread transfer | REQUIRED | One conditional frame | `src/project_semantic_dispatch/query_frames.rs` | always |
| `OPERAND_PREPARATIONS`, `conditional_operand_preparations_for_tests` | None; tests count the actual preparation entry on each polling thread to detect replay | OPTIONAL | One test thread | `src/project_semantic_dispatch/conditional_decision.rs` | `cfg(any(test, feature = "test-support"))` |
| `SemanticTask::poll_read_for_tests` | None; tests compare the terminal read's completeness, diagnostics and receipt with the synchronous entry without requiring cache admission | OPTIONAL | Derived from one poll; no extra retained state | `src/project_semantic_dispatch/query_frames.rs` | `cfg(any(test, feature = "test-support"))` |

No production decision reads the counter. A default build contains neither
its storage nor its increment or reader. No history or trace is added.
