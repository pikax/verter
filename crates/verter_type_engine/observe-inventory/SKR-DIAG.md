# Checker diagnostic recipe and occurrence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CheckerDiagnosticRecipe` on `ResolvedCallResult::Selected.recovery_diagnostic` and `ResolveCallFailure::Diagnosed` (the code, its typed `CheckerDiagnosticArguments` and the `CheckerDiagnosticSubject` position, never a source offset) | The flow-return frame binds it to the call site that reads the result, so one cached call result reports at every site that reuses it; its argument nodes are retained with the result | REQUIRED | The call result it rides | `src/semantic_query/checker_diagnostic.rs` | always |
| `CheckerDiagnosticOccurrence` list on `FlowReturnResult.checker_diagnostics` (start offset, code, printed message arguments, in the checker's order) | `FlowReturnResult::checker_diagnostics` read through `VerterHost::get_flow_return_type_with_audit`: the code, full message and authored position of each diagnostic of the body's calls | REQUIRED | The flow-return result | `src/semantic_query/flow_return_result.rs` | always |
| Frame call report positions (`call_diagnostic_sites`, by call span: the callee's report point, each argument's start, each expression-bodied unannotated arrow argument's returned-expression start) | `record_call_diagnostic` binds a recipe's subject to this call's positions; dropped with the frame | REQUIRED | One flow-return evaluation | `src/project_semantic_dispatch/flow_return.rs` | always |
| `QueryError::CheckerRecovery.origin` of a TS2456 recovery (the circular alias's `DeclRef`) | `ProjectSemanticDispatch::checker_recovery_message` names the alias in the formatted TS2456 | REQUIRED | The recovery node | `src/semantic_query/checker_policy.rs` | always |

## verter_type_expr

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `IndexedValueCall.report_point` and `IndexedValueCallArg.return_point` (parse facts: where the checker reports a whole-call diagnostic, and an arrow argument's returned expression) | The flow-return frame's call report positions above | REQUIRED | The indexed call record | `src/indexed_expression.rs` | always |

No production counter, trace or charge history is added, and no failed relation records a history: an elaboration is computed only for an argument whose candidate has already failed it, from the two types the failure names, so nothing is gated behind `semantic-observe`.
