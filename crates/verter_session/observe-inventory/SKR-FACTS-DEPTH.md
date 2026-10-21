# Connected query-depth refusal on the public component-meta routes

No field, store, hook, counter or trace was added or removed. The rows below
record the typed cause paths whose behavior changed.

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `adopt_projection_result` causes for an unavailable `ResolveDecl`/`Instantiate` miss after incomplete work (`unavailable_causes`, `lookup_cut_short`) | Memo settlement delivers the stop's observed causes only; a query-depth stop stays `CONNECTED_QUERY_DEPTH_LIMIT` instead of gaining `MISSING_DEPENDENCY` | REQUIRED | One settled read | `project_semantic_dispatch/projection_fact` | always |
| `ShallowDiagnostic::UnresolvedSurfaceArm` for a `DeclRef` arm | Not recorded when the arm's lookup returned `IncompleteSemanticOperand`; the read already folded its causes, so the macro surface does not add `MISSING_DEPENDENCY` for a stopped lookup | REQUIRED | One shallow walk | `project_semantic_dispatch/walk` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `deep_partial_chain_source`, `isolated_depth_refusal_host` | Owner-test fixture and public invocation that trip the production query-depth bound through `get_component_meta` | test-only | Test | `host_manage/component_meta_entry_depth_tests` | cfg(test) |
