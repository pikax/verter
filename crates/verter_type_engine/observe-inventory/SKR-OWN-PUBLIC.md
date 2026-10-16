# Public semantic carrier inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Production canonical sweeps, ordinal watermarks and `SemanticActivityGate`
remain the close/reload authority: a public root does not promise survival
across a legacy canonical close (it then reads as `StaleSemanticOperand`).
Counted last-root destruction stays inactive in production.

## Reachable public carriers

Every row names how a consumer outside the engine reaches the carrier, the rule
that keeps its graph operands owned or borrowed, and the evidence that holds it.
Compile contracts live in `crates/verter_session/tests/cases/compile-fail/`
(run by `node scripts/compile-contracts.mjs`, `session` owner, production
features); runtime proofs run in the counted ownership harness.

| Carrier | Extraction / clone / read route | Ownership / lifetime rule | Proof |
| --- | --- | --- | --- |
| `SemanticNodeId` | Copied freely from any payload, value or diagnostic | An identity label; no production API outside the engine reads payload from it | `public_result_and_payload_cannot_be_extracted` (`node_data`, `node_data_for` private) |
| `NodeRef<'a>` | Minted only by an owner (root, root set, payload lease child, delivery, signature value, reduced/materialized output, construction scope) | Borrowed, arena-qualified, cannot outlive its owner; private fields | `public_node_capability_cannot_be_forged` (E0451), `public_node_capability_cannot_outlive_owner` (E0515 / lifetime); `foreign_and_unowned_capabilities_fail_safely` |
| `NodeRoot` / `NodeRootSet` | `NodeRef::to_root`, `root_in_scope`, host typeinfo results | Owning; clones share one acquisition; equality is node + graph | `a_member_child_lease_outlives_its_parent_read`, `a_resolved_root_projects_after_its_request_and_caches_are_gone` |
| `NodeDataLease` | `NodeRef::read`, `SemanticGraphStore::read`, `read_in_scope` | Owning; derefs to a borrowed `SemanticNodeData`; no conversion to an unleased `Arc` or owned payload | `public_result_and_payload_cannot_be_extracted` (`Arc` / copy legs); `delivered_values_members_and_cloned_reads_outlive_every_producer_owner` |
| `SemanticNodeData` and every variant child | Borrowed from a lease; children reached by `NodeDataLease::child` | No public copy (crate-private exhaustive copy); a child ordinal not retained by the payload is foreign | `public_result_and_payload_cannot_be_extracted`; `foreign_and_unowned_capabilities_fail_safely` |
| `SurfaceView` (`entries`, `members`, `call_signatures`, `construct_signatures`, `index_signatures`, `keyspace`) | Borrowed from the payload lease; slice accessors | No public copy; nested arrays never exposed as `Arc` | `public_surface_nested_carriers_cannot_detach` |
| `SurfaceMember` / `SurfaceEntry` / `IndexSignature` | Borrowed from the surface; builder copies via `copy_in_scope` | No public copy; a builder copy roots its operands in the active construction scope | `public_surface_nested_carriers_cannot_detach`, `public_construction_copy_is_store_only` |
| `SurfaceLease` | `ProjectSemanticDispatch::resolve_typeinfo_surface` | Owning read of the terminal `Object`; derefs to the borrowed surface | `a_delivered_surface_lease_owns_its_members`; `public_owned_carriers_borrow_and_own` (pass control) |
| `CacheRead<T>` value, lease, dependency signature, diagnostics, receipt | Borrowed accessors; `CacheRead::node` borrows an ordinal the delivery roots | Fields crate-private; a clone clones the lease; dependency signatures hold file versions, never graph operands | `public_result_and_payload_cannot_be_extracted` (`value`, `walker_diagnostics` legs); `delivered_values_members_and_cloned_reads_outlive_every_producer_owner` (cold, warm, retained, zero-headroom ReturnOnly) |
| `ShallowDiagnostic` and its operands | Borrowed from the delivering read; operands via `CacheRead::node` | No public copy; owned by the delivering read and every clone of it | `public_diagnostic_error_cannot_detach`; `diagnostic_operands_stay_owned_by_the_delivering_read` |
| `DiagnosticError` (nested `QueryError`, recursive-reference arguments, recovery operands) | Rendered through `Debug`; `is_fatal` | No public copy and no borrowed `QueryError` view | `public_diagnostic_error_cannot_detach` |
| Recursion carriers (`QueryResult::Recursive`) | Delivered to a re-entrant reader | Owned by that reader's read | `a_recursion_delivery_owns_its_carrier` |
| `SignatureSetValue` / `SignatureCandidateNodes` | `signature_set_value`, query values; `candidate_nodes()` borrowed; `SignatureSetValue::node` | Every clone owns the candidate nodes (table and roots share one `Arc`); candidate fields private | `public_signature_children_cannot_detach`; `a_cloned_signature_set_owns_its_candidate_nodes_through_its_final_holder` |
| Type-token reads | `SignatureStore::type_token_root`, `SemanticReadView::type_token_root` | Owning root from the epoch's token roots; ordinal accessors crate-private | `public_signature_children_cannot_detach` (`type_token_node` leg) |
| `ReducedOutputNode` / `MaterializedOutputTypeExpr` | `node()` capability | Roots held by the output; ordinal kept as identity metadata | `a_reduced_output_owns_its_node`; the materialized output shares the same root attachment its sealed payload acquires |
| Interned identity operands (substitutions, recipes, relation/call keys) | Copied with the identity | Each copy shares the issuing arena's root attachment (`IdentityNodeRoots`) | `counted_identity_record_and_prefix_receipt_own_nonempty_operands`, `counted_equal_recipes_in_distinct_arenas_do_not_share_nested_owners` |
| Query keys holding ordinals | Construction lane: dispatch `execute` | Prepared identities root key operands; a dispatcher's construction scope owns results until it drops | Prepared-identity bijection suite; full session suite under the dispatch scope |
| Construction-domain ordinal reads (session producers, audit miners) | `read_in_scope`, `root_in_scope`, `copy_in_scope` | Answer only while the thread holds a construction scope of that graph; return rooted leases | Session suite (a read outside any scope answers nothing) |
| Host typeinfo results (`resolve_named_symbol*`, `evaluate_type_expression_with_audit`) | Return `NodeRoot` | Owning across requests; projections validate the root's graph | `a_resolved_root_projects_after_its_request_and_caches_are_gone` |
| NAPI / WASM resolve-then-project | `split_resolve_outcome` carries the `NodeRoot` into `project_node_to_type_expr_json_bytes(&root)` | The second call reads through the root the first call returned | Same test (foreign root projects nothing; planting a bare-ordinal projection fails it) |

## Fields, stores and hooks

| Fields, stores and hooks | Consumer | Classification | Lifetime / owner | Gate |
| --- | --- | --- | --- | --- |
| `NodeRef::{id, arena}` | Every capability read | REQUIRED | Borrow of its owner (`semantic_query_memo/node_roots.rs`) | always |
| `NodeRootScope::core`, scope `holds` / `node_ref` | Construction-lane reads and builder copies | REQUIRED | Scope lifetime, thread-bound | always |
| `ProjectSemanticDispatch::_construction` | Dispatcher-owned construction scope | REQUIRED | Dispatcher lifetime (`project_semantic_dispatch/mod.rs`) | always |
| `SignatureSetValue::nodes` (`CandidateTable { candidates, roots }`) | Signature consumers | REQUIRED | Last value copy (`signature_kernel/result.rs`) | always |
| `DiagnosticError` | Walker diagnostics | REQUIRED | Owning diagnostic (`project_semantic_dispatch/walk.rs`) | always |
| `SurfaceLease` | Typeinfo surface consumers | REQUIRED | Last lease clone (`semantic_query_memo/node_data_lease.rs`) | always |
| `construction_copy::{ConstructionCopy, sealed::CopyToken}` | Session builders | REQUIRED | Call duration; operands rooted in the active scope | always |
| `GraphOwnership` and `VerterHost::new_standalone_with_counted_ownership_for_test` | Counted ownership proofs over a whole host | REQUIRED-test | Fixture host lifetime | `test` or `test-support`; `counted_ownership_host_is_not_production` |
| `copy_for_tests`, `CacheRead::unleased_for_tests`, `DiagnosticError::for_tests`, public ordinal read seams | Fixtures | REQUIRED-test | Fixture lifetime | `test` or `test-support` |

No ownership history, delivered-capability registry or observation record is
added. Capability reads take one root acquisition per lease; required current
counts (`semantic_node_*`) are unchanged.
