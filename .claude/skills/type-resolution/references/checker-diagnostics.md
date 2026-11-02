# Checker diagnostic recipes and occurrences

A diagnostic the checker reports for a type operation is split in two.

**Recipe** (`CheckerDiagnosticRecipe`, `semantic_query/checker_diagnostic.rs`): the code, its typed
`CheckerDiagnosticArguments` (graph nodes or counts, never printed text), a `CheckerDiagnosticSubject`
— a position of the operation (`Operation`, `Argument(n)`, `ExcessArguments(n)`, `n` an AUTHORED
argument ordinal), never a source offset — and `last_overload` (the recipe is the error the last of
several overloads gave, reported as TS2769). Call resolution produces recipes
(`ResolvedCallResult::Selected.recovery_diagnostic`, `ResolveCallFailure::Diagnosed`), so a cached
call result's recipe is valid at every call site that reuses it. Its argument nodes are retained with
the result (`retained_nodes.rs`). A tuple spread's expanded arguments carry the spread's authored
ordinal (`CallArgument.authored`).

**Occurrence** (`CheckerDiagnosticOccurrence`): a recipe bound to one authored site
(`CheckerDiagnosticRecipe::bind`) — the byte offset it starts at, the code, the message arguments
printed for good, and its ordered message chain (`CheckerDiagnosticChainLine`: depth + message). It
names no graph node. `FlowReturnResult::checker_diagnostics` carries the body's occurrences,
deduplicated and in the checker's order (position, code, message, chain); `occurrence.message()` is the
diagnostic's own message, `occurrence.full_message()` the message with its chain as
`tsc --pretty false` prints it.

## Binding

The flow-return frame records each executor-routed call's report positions by call span
(`call_diagnostic_sites`): the callee report point (`IndexedValueCall.report_point` — a property
access's name, else the callee, the whole expression for `new` and tagged templates) and every authored
argument's `IndexedReportSite` (`IndexedValueCallArg.report`: its start — a spread's `...` — and its
`IndexedElaborationForm`). `record_call_diagnostic` binds against them; a call with no recorded
positions, a subject the positions do not hold, or an elaboration that is not modelled records NOTHING
— a diagnostic is never moved to another position or given a message the checker's is not.

A TS2345 recipe bound at its argument goes through `CheckerDiagnosticPrinter::relation_failure` →
`ProjectSemanticDispatch::checker_relation_failure` (`call_resolve_elaboration.rs`), the checker's
`checkTypeRelatedToAndOptionallyElaborate`:

1. `elaborateError` over the authored form: an unannotated expression-bodied arrow reports its return
   against the union of the target's call-signature returns as TS2322 at the returned expression; an
   object literal reports each property whose type fails the target's as TS2322 at its name; an array
   literal each failing element. Forms recurse (an arrow returning an object literal reports at the
   property). `IndexedElaborationForm::Unmodelled` (spreads, computed names, `as const`, holes) binds
   nothing.
2. A fresh object literal's first property the target names nowhere: TS2353, or TS2561 with the
   spelling suggestion (`getSpellingSuggestion`).
3. One diagnostic at the argument: TS2345 — or, where the pair itself lacks required properties,
   TS2741/TS2739/TS2740 in its place — with the relation's message chain.

The chain explains the failure step by step, asking the SHARED relation (`execute_relate_pair`) which
child of each pair fails, in the checker's order: a union source's first failing member, an
intersection target's first failing member, array element types, tuple positions, missing properties
(which replace their pair's own line), property types (a run of properties and signature returns prints
as one path, `The types of 'a.b' …` / `The types returned by 'f()' …`), parameters (`Types of parameters
…`), type predicates, signature returns. Each pair's own line generalizes a literal source as
`reportRelationError` does (`print_relation_operands`). A step this does not model — a non-primitive
source against a target union (best match), index signatures, generic, optional or rest signature
parameters, an undecided relation — explains nothing, and the diagnostic is not reported. No relation
records a history: the explanation runs only when a recipe is bound. Its depth is the checker's relation
stack (`checker_policy::RELATION_STACK_DEPTH`, 100): every step into a structured pair's part — a
property, an array element, a tuple position, a parameter, a return, a predicate — holds the pair open
on it; so does an intersection target's pair, a union source's pair unless it has fewer than four
constituents (`boolean` counting two) against a non-union target (`isRelatedTo`'s `skipCaching`), and the
failing pair itself when it lacks properties or either side is structured or instantiable — never two
primitives, nor an object type against a primitive (`isRelatedTo` decides that pair before
`recursiveTypeRelatedTo`, and the relation engine's `checker_recursion_entry` agrees). Past it the
checker does not relate the pair at all: it answers `Maybe`, which holds, and nothing is reported — not
at the call, not at the argument, elaborated or under overloads — while the rest of the relation is
related as usual (TypeScript 7.1.0-beta). This lane reports nothing there either; the relation
engine itself still fails the pair past the depth (the known deviation documented on
`CHECKER_RELATION_DEPTH_LIMIT`), so such a call is a typed gap rather than a complete answer. When the two printed operands
of an elaborated assignment coincide, its head is TS2719, retaining the complete failure chain.
`relation_type_names` owns that decision for every message naming a failed pair, as `getTypeNamesForErrorDisplay` does: heads and chain lines through `relation_operands`, and the missing-property messages (TS2741, TS2739, TS2740) directly. Two operands that print the same text are printed again fully qualified (`DisplayFacet::QualifyNames` with `CheckerSpelling`), and only spellings that still agree take TS2719's message. Each declaration's symbol chain comes from `declaration_chain`: a module export prints as `import("<file without extension>").<export>`, an `export =` declaration as `import("<file without extension>")` with any namespace path below it, while a global or a declaration its module does not export prints bare, so same-named types from two modules keep TS2322 and unexported ones keep TS2719. The declaration chain retains its lexical owner: an export in a different script owner cannot qualify a same-named local declaration. A private namespace member stops the chain at the last private segment (`Hidden`, or `B.Inner` below private namespace `B`), using the shared header visibility inventory. A chain that cannot be determined withholds the diagnostic. A generalized literal source is always fully qualified (`getTypeNameForErrorDisplay`), collision or not: `literal_base_spelling` settles carriers and prints an enum base through its chain, so an exported enum passed to `string` names `import("<file>").E`.

Types print as the checker's `typeToString` prints them (`print_diagnostic_type`,
`DisplayContext::for_checker`): the display walk keeps the node builder's running `approximateLength`
(UTF-8 bytes, per node kind as the node is built: a named reference counts its name twice after its
type arguments, a property its name and colon before its type, an index signature its key and value
types then its parameter name and four, a tuple element its type alone (a label adds nothing), an
object its braces after its members, each list entry two bytes before it) and its truncation latch past `checker_policy::MAXIMUM_TRUNCATION_LENGTH` (160) — a union's
or an intersection's remaining members then print as `... N more ...` before the last, an object's
remaining properties likewise, an object type it begins past the length as `...`, a type-argument or
tuple list as `...` — and the whole text is clipped to its first 317 bytes and `...` at 320. An object
with no property or index signature and exactly one call (or one construct) signature prints as that
signature's function (or constructor) type (`createTypeNodeFromObjectType`): a `typeof` of a declared
function prints `() => void`, never `{ (): void; }`. An object
prints its call, construct and index signatures before its properties, an index signature by its
declared parameter name (`IndexSignature::key_name`; `x` for one no declaration wrote), and under
`strictNullChecks` an optional tuple element's type with `undefined` added. A query (`typeof a.b`) and a class-expression instance (`(Anonymous class)`)
count their symbol chain — each name and separator, the first twice — after their type arguments; an
import type (including a fully qualified module export) its specifier and ten, then each qualifier name and separator once; a type predicate its type
alone. A part with no checker type (an `Opaque` error carrier) is cut. Generic mapped types count their key constraint, parameter name, remap and template
in builder order, then ten bytes for the mapping. Closed mappings, spread
alternatives and anonymous peer-merged declarations are prepared by the consumer
before display; display itself never evaluates. The shared spread evaluator
composes each nested construction's evaluated type before its parent. Generic
spreads print ordered intersections with closed property groups. Canonical
intersection construction deduplicates repeated generic operands across nested
intersections; a non-generic right operand merges only with the left
intersection's final non-generic group, through the shared spread producer.
Independent generic right operands retain their own groups, including property
conflicts. The canonical discriminant reduction recognizes `boolean` as the
implicit union of literal types, so conflicting groups can reduce to `never`.
Primitive-target relations read the same evaluated type, preserving bottom
assignability. A spread of `any` retains the ordinary dynamic relation. Open positive evidence is never
interned as a closed object. An authored `infer` constraint
prints with `extends` unless its position already infers that constraint (a
template span, rest element/parameter, mapped key or a mapped value matched
against a mapped check type). Multiple inference sites contribute their
intersection. The consumer compares that canonical inferred constraint with the
authored constraint through the shared identity relation after ordered use-site
substitution; display only reads the resulting decision. Its name is counted again
as a parameter declaration, once rather than as a reference. Unique-symbol
indexed-access types print `unique symbol`.

Computed property names use the same consumer preparation worklist: non-nominal
`typeof` keys evaluate through the shared owner and enum-member keys use their
literal base. Display reads these settled forms without dispatching queries;
unique-symbol keys retain their nominal spelling and deterministic symbol count.

A symbol-keyed property's abbreviation contribution (its name and colon) is the UTF-8 byte
length of its full dotted spelling, its leaf spelling again and twenty bytes: `[s0]` counts 24,
`[ns.q0]` 27, `[deep.er.q0]` 32. Both unique-symbol keys and computed keys resolved to a nominal
symbol use this rule; qualification remains in the visible `[namespace.symbol]` spelling. No
allocation id, external checker session or query history contributes to the count, and none
contributes to the checker's either: TypeScript 7.1.0-beta abbreviates a symbol-keyed type the same
with and without unrelated symbol work first (`node scripts/checker-symbol-history.mjs` proves it
with fresh and prewarmed native sessions under both nullability settings), so the print is the
checker's exactly. Printable symbol-keyed types always retain their diagnostic, including at
abbreviation boundaries. `symbol_abbreviation_keeps_diagnostics_across_query_histories` checks full
consumer messages and recovery cold/warm, with unrelated symbol work first and at two authored call
sites.

### Unprintable carriers

Unresolved computed keys and non-type error carriers likewise remain outside a
faithful print. A depth-cap cut inside the kept text still withholds the message.

Decided by the recipe's producer: arity errors from `ArityEvidence` (`getArgumentArityError`) — TS2575
for a count in a gap between the candidates' arities, else TS2554/TS2555 with `min`, `min-max` or the
rest-required count, too many reported from the first authored argument no candidate accepts, too few
and TS2575 at the callee. In the contextual route, several arity-fitting candidates report the LAST
one's error as TS2769 at its position (`The last overload gave the following error.` plus that error
below it); an eager argument's failure found before an earlier callback was typed is not known to be
the checker's first failure and records no diagnostic.

## Routing

- A lone non-generic signature with a DECLARED return is resolved through the executor
  (`lone_declared_signature`), so its applicability — and its diagnostic — is decided; its answer
  is the declared return either way: a refusal, or a route that cannot type the call, falls back to
  the caller's declared read (`callee_is_lone_declared`).
- With explicit type arguments over overloads, the one candidate whose arity fits the call reports
  its argument's TS2345 instead of TS2769, and the call recovers with the checker's longest candidate
  (`explicit_overload_recovery`, `getLongestCandidateIndex`) — the same one, or another whose
  instantiated return the call then takes, carrying the fitting candidate's diagnostic.

## Formatted node and walker diagnostics

- `ProjectSemanticDispatch::checker_recovery_message(node)`: the `TS<code>: <message>` a
  `QueryError::CheckerRecovery` node carries; TS2456 names the alias its origin `DeclRef` refers to.
  `checker_recovery_location(node)` is where the checker reports it: the alias's name span from its
  header. The resolve-symbol typeinfo graph (`resolve_symbol_graph_with_audit`, `include_diagnostics`)
  carries both as a `GraphDiagnostic`.
- `ProjectSemanticDispatch::checker_message(&ShallowDiagnostic)`: TS2859 naming both types; the
  component-meta expansion diagnostic's context carries it.

Coverage: `call_diagnostics_carry_the_checkers_code_message_position_and_order` (TypeScript 7.1.0-beta
lines with their chains, cold and warm, two call sites),
`long_messages_print_as_the_checker_abbreviates_them` (a wide union, a wide object, a 70-level
property path, reference arguments, index signatures and a long tuple label at their abbreviation
boundaries, cold and warm, two call sites of one body),
`messages_print_tuple_elements_and_object_members_as_the_checker_does`,
`wide_arguments_of_queried_and_class_expression_types_print_as_the_checker_abbreviates_them` (wide
function-query, qualified-query, class-expression and predicate-method objects at their elision and
clip, cold and warm),
`deep_chains_report_until_the_checker_relation_stack_overflows` (the relation stack's boundary per
leaf kind, nothing reported past it), `elaborated_and_overloaded_overflows_report_nothing`,
`call_diagnostics_follow_the_checkers_optionality_union_order_and_nullability` (the explicit-overload
recovery with a differing return), `a_diagnostic_the_route_cannot_place_is_not_reported`, the `checker_diagnostic::tests` recipe-binding
units, `resolve_symbol_graph_reports_the_checkers_ts2456_at_the_alias_name`,
`a_relation_refusal_reports_the_checkers_formatted_ts2859`,
`a_directly_circular_generic_alias_is_the_checkers_ts2456_under_any_budget`.
