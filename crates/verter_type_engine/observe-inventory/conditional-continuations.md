# Conditional continuation state

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The interface is described in [root continuations](../../../docs/arch/root-continuations.md).

| Item | Production consumer | Classification | Lifetime | Owner module | Gate |
| --- | --- | --- | --- | --- | --- |
| `ConditionalDecisionState.prepared`, `PreparedConditionalOperands` | The decision resumes selection from the operands already prepared; their adopted observations remain in the owning frame's taint and tracer | REQUIRED | One conditional frame, or one synchronous decision | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `ConditionalDecisionState.selection_started`, `ConditionalDecisionState.distribution`, `ConditionalDistribution` | The decision retains absorption/distribution classification and the completed member prefix, ordered results, self roots and positional completeness across dependency delivery | REQUIRED | One conditional frame, or one synchronous decision | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `ConditionalDistribution.normalization_members` | Prepare the final reduction's canonical member order once and retain its memo identity across dependency delivery | REQUIRED | One distribution | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `ConditionalDecisionState.selected`, `ConditionalSelection` | A completed relation/inference verdict, bindings and false-recovery classification survive a selected winner's dependency wait; adopted observations remain in the frame | REQUIRED | One conditional frame, or one synchronous decision | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `ConditionalBuild.decision` | The query frame owns the decision's sequential position across dependency delivery and thread transfer | REQUIRED | One conditional frame | `src/project_semantic_dispatch/query_frames.rs` | always |
| `DecisionSeam.answers`, `DecisionSeam.needs` for union reduction | Transfer a distributed conditional's final reduction as a child demand and consume its delivery without repeating cold-query admission | REQUIRED | One conditional frame, installed during each step | `src/project_semantic_dispatch/query_frames.rs` | always |
| `DecisionSeam.materialized_children`, `ConditionalChildQueries` | Restrict child transfer to explicit materialized members, inferred winners and final union reductions; other probes retain their checker context | REQUIRED | One decision step, scoped to the child operation | `src/project_semantic_dispatch/query_frames.rs` | always |
| `OPERAND_PREPARATIONS`, `conditional_operand_preparations_for_tests` | None; tests count the actual preparation entry on each polling thread to detect replay | OPTIONAL | One test thread | `src/project_semantic_dispatch/conditional_decision.rs` | `cfg(any(test, feature = "test-support"))` |
| `DISTRIBUTED_MEMBERS`, `conditional_distributed_members_for_tests` | None; tests count completed member folds at the production fold to distinguish retained traversal state from repeated reads of memo answers | OPTIONAL | One test thread | `src/project_semantic_dispatch/conditional_decision.rs` | `cfg(any(test, feature = "test-support"))` |
| `BRANCH_SELECTIONS`, `conditional_branch_selections_for_tests` | None; tests count entries to the relation/inference selection stage and detect completed selections repeated during winner suspension, including replay shared by synchronous and polled entries | OPTIONAL | One test thread | `src/project_semantic_dispatch/conditional_decision.rs` | `cfg(any(test, feature = "test-support"))` |
| `SemanticTask::poll_read_for_tests` | None; tests compare the terminal read's completeness, diagnostics and receipt with the synchronous entry without requiring cache admission | OPTIONAL | Derived from one poll; no extra retained state | `src/project_semantic_dispatch/query_frames.rs` | `cfg(any(test, feature = "test-support"))` |

No production decision reads the counter. A default build contains neither
its storage nor its increment or reader. No history or trace is added.
