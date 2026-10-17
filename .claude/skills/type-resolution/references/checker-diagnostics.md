# Checker diagnostic recipes and occurrences

A diagnostic the checker reports for a type operation is split in two.

**Recipe** (`CheckerDiagnosticRecipe`, `semantic_query/checker_diagnostic.rs`): the code, its typed
`CheckerDiagnosticArguments` (graph nodes or counts, never printed text) and a
`CheckerDiagnosticSubject` — a position of the operation (`Operation`, `Argument(n)`,
`ExcessArguments(n)`), never a source offset. Call resolution produces recipes
(`ResolvedCallResult::Selected.recovery_diagnostic`, `ResolveCallFailure::Diagnosed`), so a cached
call result's recipe is valid at every call site that reuses it. Its argument nodes are retained
with the result (`retained_nodes.rs`).

**Occurrence** (`CheckerDiagnosticOccurrence`): a recipe bound to one authored site
(`CheckerDiagnosticRecipe::bind`) — the byte offset it starts at, the code, and the message
arguments printed for good. It names no graph node. `FlowReturnResult::checker_diagnostics`
carries the body's occurrences, deduplicated and in the checker's order (position, code, message);
`occurrence.message()` is the full message (`CheckerDiagnosticCode::format`).

## Binding

The flow-return frame records each executor-routed call's report positions by call span
(`call_diagnostic_sites`): the callee report point (`IndexedValueCall.report_point` — a property
access's name, else the callee, the whole expression for `new` and tagged templates), every
argument's start, and each argument's returned-expression start (`IndexedValueCallArg.return_point`
— an arrow whose body is one expression and which annotates no parameter). `record_call_diagnostic`
binds against them; printing goes through `CheckerDiagnosticPrinter` (display with
`DisplayFacet::CheckerSpelling`: double-quoted string literals, `;`-terminated object members).

Decided at bind time, because they depend on the authored form and not on the types:

- `ElaboratedReturn` arguments: an argument whose function value's return fails is the returns'
  TS2322 at the returned expression when the site's argument is such an arrow
  (`elaborateArrowFunction`), the argument's TS2345 anywhere else.
- `MissingProperties` arguments: TS2741 for one missing required property, TS2739 for two to five,
  TS2740 (first four named) beyond (`reportUnmatchedProperty`).

Decided by the recipe's producer: TS2345 prints a literal source as its base primitive unless the
target could hold a literal (`print_relation_operands`, `reportRelationError`); arity errors are
TS2554/TS2555 with `min`, `min-max` or the rest-required count, too many reported from the first
excess argument, too few at the callee (`argument_count_recipe`).

## Routing

- A lone non-generic signature with a DECLARED return is resolved through the executor
  (`lone_declared_signature`), so its applicability — and its diagnostic — is decided; its answer
  is the declared return either way, and a refusal falls back to the caller's declared read
  (`callee_is_lone_declared`).
- With explicit type arguments over overloads, the one candidate whose arity fits the call, when it
  is also the checker's longest-candidate pick, is the error-recovery candidate
  (`sole_arity_fitting_candidate`), reporting its argument's TS2345 instead of TS2769.

## Formatted node and walker diagnostics

- `ProjectSemanticDispatch::checker_recovery_message(node)`: the `TS<code>: <message>` a
  `QueryError::CheckerRecovery` node carries; TS2456 names the alias its origin `DeclRef` refers to.
- `ProjectSemanticDispatch::checker_message(&ShallowDiagnostic)`: TS2859 naming both types; the
  component-meta expansion diagnostic's context carries it.

Coverage: `call_diagnostics_carry_the_checkers_code_message_position_and_order` (TypeScript 7.0.2
lines, cold and warm, two call sites), `a_relation_refusal_reports_the_checkers_formatted_ts2859`,
`a_directly_circular_generic_alias_is_the_checkers_ts2456_under_any_budget`.
