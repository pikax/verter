# Checker diagnostic recipe and occurrence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CheckerDiagnosticRecipe` on `ResolvedCallResult::Selected.recovery_diagnostic` and `ResolveCallFailure::Diagnosed` (the code, its typed `CheckerDiagnosticArguments`, the `CheckerDiagnosticSubject` position — an authored argument ordinal, never a source offset — and whether it is a last overload's error reported as TS2769) | The flow-return frame binds it to the call site that reads the result, so one cached call result reports at every site that reuses it; its argument nodes are retained with the result | REQUIRED | The call result it rides | `src/semantic_query/checker_diagnostic.rs` | always |
| `CheckerDiagnosticOccurrence` list on `FlowReturnResult.checker_diagnostics` (start offset, code, printed message arguments and the ordered `CheckerDiagnosticChainLine` message chain, in the checker's order) | `FlowReturnResult::checker_diagnostics` read through `VerterHost::get_flow_return_type_with_audit`: the code, full message with its chain, and authored position of each diagnostic of the body's calls | REQUIRED | The flow-return result | `src/semantic_query/flow_return_result.rs` | always |
| `CallArgument.authored` (the authored argument each expanded call argument is — a tuple spread's, for every argument it expands to) | The recipe's `Argument`/`ExcessArguments` subject, so a diagnostic binds to the authored argument the checker reports at | REQUIRED | One call resolution | `src/project_semantic_dispatch/call_resolve.rs` | always |
| `ContextualCallState.arity` (`ArityEvidence`: the fewest required, the most declared, the nearest counts below and above the call's, and whether a rest exists, over the candidates whose arity the call fails) | `ContextualCallState::failure` builds the arity error from it when no candidate fits: TS2575 in a gap, else TS2554/TS2555 | REQUIRED | One contextual call route | `src/project_semantic_dispatch/call_resolve_contextual.rs` | always |
| `ContextualCallState.typed_arguments` (a callback's once-assigned contextual typing per argument position, kept by the first candidate that types it) and `ContextualCallState.candidate_typed_arguments` (a candidate's own contextual typing, per candidate and position) | The deferred loop relates each candidate by the once-assigned typing (its applicability) and reports the recipe from the candidate's own typing, so the last overload's error names the callback as its own parameters typed it | REQUIRED | One contextual call route | `src/project_semantic_dispatch/call_resolve_contextual.rs` | always |
| `LoopPassMark.checker_diagnostics` / `LoopPassTail.checker_diagnostics` (the frame's occurrence list length at a loop pass's start, and the occurrences the pass produced beyond it) | `set_aside_loop_pass` rewinds a provisional loop-head pass's occurrences with its other side outputs and `replay_loop_pass` puts them back when that pass is the converged one, so a call in a loop reports each occurrence once | REQUIRED | One loop pass | `src/project_semantic_dispatch/flow_return.rs` | always |
| Frame call report positions (`call_diagnostic_sites`, by call span: the callee's report point and each authored argument's `IndexedReportSite`) | `record_call_diagnostic` binds a recipe's subject to this call's positions; a call with none records no diagnostic; dropped with the frame | REQUIRED | One flow-return evaluation | `src/project_semantic_dispatch/flow_return.rs` | always |
| `QueryError::CheckerRecovery.origin` of a TS2456 recovery (the circular alias's `DeclRef`) | `ProjectSemanticDispatch::checker_recovery_message` names the alias in the formatted TS2456 and `checker_recovery_location` reads the alias's name span off its header; the resolve-symbol typeinfo graph carries both as a `GraphDiagnostic` | REQUIRED | The recovery node | `src/semantic_query/checker_policy.rs` | always |

## verter_type_expr

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `IndexedValueCall.report_point` and `IndexedValueCallArg.report` (parse facts: where the checker reports a whole-call diagnostic; each argument's report point — a spread's `...` — and its `IndexedElaborationForm`: an unannotated expression-bodied arrow's returned expression, an object literal's named members, an array literal's elements, or a form not modelled) | The flow-return frame's call report positions above, and the elaboration that reports a failure into an argument's parts | REQUIRED | The indexed call record | `src/indexed_expression.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SemanticTypeGraph.diagnostics` on a resolve-symbol answer (an existing wire field: the root's checker diagnostic as a `GraphDiagnostic` with its message and span interned in the graph's strings) | `VerterHost::resolve_symbol_graph_with_audit` with `include_diagnostics` | REQUIRED | The response | `src/typeinfo/graph_query.rs` | the request's `include_diagnostics` |

No production counter, trace or charge history is added, and no failed relation records a history: a message chain is computed only when a recipe is bound to its occurrence, from the two types the failure names, by asking the shared relation which child of each pair fails, so nothing is gated behind `semantic-observe`.
