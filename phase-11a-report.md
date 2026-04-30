# Phase 11a — meta_resolve.rs split (continuation)

## Summary

Phase 11a splits the 12,745-line `meta_resolve.rs` god module into a folder
module with thirteen private siblings (plus a thin shell) under
`crates/verter_session/src/meta_resolve/`. Every production sibling is
below the 4,000-line `god_module_size_budget`. Public-API surface is
preserved verbatim — external `crate::meta_resolve::*` paths continue to
resolve through the shell's `pub(crate) use sibling::*;` re-exports.

Work landed in 9 NEW commits on top of the 6 foundation commits the
original worker landed (15 commits total on the branch). The
continuation worker discarded the prior worker's broken
string-concatenation patch on `phase_05m_class_b_callers_migrated_through_bridge_helpers`
and redesigned that test to a robust walkdir-based per-file scan
(commit 7), then cleanly redid the materialize/ sub-split (commit 8) and
landed the remaining six per-domain extractions (commits 9–14) +
marker (commit 15).

## Sibling roster (post-Phase-11a)

| Sibling                                      | LOC   | Domain                                                          |
|----------------------------------------------|-------|-----------------------------------------------------------------|
| `meta_resolve.rs` (shell)                    | 148   | Module declarations + `pub(crate) use` re-exports               |
| `meta_resolve/dep_signature.rs`              | 198   | Domain 6 — DISPATCH_DEP_SIGNATURE_ACCUMULATOR + BFS counters   |
| `meta_resolve/dispatch_helpers.rs`           | 649   | Domains 1+2 — Phase 5d dispatch helpers + 5l bridges            |
| `meta_resolve/field_state.rs`                | 185   | Domain 3 — MacroFieldGraphState                                 |
| `meta_resolve/graph_predicates.rs`           | 1271  | Domain 12 — graph-native route/cycle-BFS predicates             |
| `meta_resolve/host_methods.rs`               | 2507  | Domain 8 — `impl VerterHost { ... }` host-method block         |
| `meta_resolve/jsdoc_resolve.rs`              | 643   | Domains 14+15 — HostComponentMetaResolver + JSDoc helpers       |
| `meta_resolve/macro_member_walk.rs`          | 815   | Domain 9 — walker + macro-shape member traversal                |
| `meta_resolve/materialize/mod.rs`            | 48    | Materialization sub-shell                                        |
| `meta_resolve/materialize/field_types.rs`    | 1796  | Stabilizer + materialize_component_meta_field_types             |
| `meta_resolve/materialize/macro_shapes.rs`   | 2338  | produce_macro_object_shapes + macro-shape synthesis             |
| `meta_resolve/origin_graph.rs`               | 194   | Domain 13 — origin-graph builder                                |
| `meta_resolve/registry_materialize.rs`       | 1158  | Domain 11 — registry structural materializer + route preservers |
| `meta_resolve/request_host.rs`               | 385   | Domain 4 — request-host adapters + cache-key helper             |
| `meta_resolve/resolved_state.rs`             | 637   | Domain 5 — ResolvedComponentMetaState + substitution helpers    |
| `meta_resolve/scoring.rs`                    | 353   | Domain 10 — symbolic-penalty + materialization-improvement     |

**Largest production sibling:** `host_methods.rs` at 2,507 lines (vs 4,000
budget). The mandatory `materialize/` sub-split bounded the materializer's
two halves at 1,796 (`field_types.rs`) and 2,338 (`macro_shapes.rs`),
both well under budget.

**Total LOC across module:** 13,325 (vs pre-split 12,745) — the +580
delta is entirely from per-sibling docstrings, `use` headers, and
`pub(crate) use` re-export blocks; zero semantic changes.

## Per-commit summary

### Foundation (commits 1–6, pre-existing on branch at spawn)

| SHA       | Step  | Subject                                                                |
|-----------|-------|------------------------------------------------------------------------|
| ce0acf2e  | 11a.1 | scoring helpers → `scoring.rs`                                         |
| f797804a  | 11a.2 | dep_signature thread-local + test counters → `dep_signature.rs`        |
| d6bff068  | 11a.3 | MacroFieldGraphState + dispatch lower counter → `field_state.rs`       |
| 2410897a  | 11a.4 | dispatch-direct surface helpers + Phase 5l bridges → `dispatch_helpers.rs` |
| 68c26fd2  | 11a.5 | resolved-state types + small substitution helpers → `resolved_state.rs` |
| 97c82a3b  | 11a.6 | request-host adapters + cache-key helper → `request_host.rs`           |

### Work commits (commits 7–15, this continuation)

| SHA       | Step | Subject                                                                  |
|-----------|------|--------------------------------------------------------------------------|
| 685e9227  | 7    | test(arch): redesign phase_05m_class_b_callers test for walkdir scan    |
| 3d778d89  | 8    | move materialization core into materialize/ submodule                   |
| 00713e42  | 9    | extract impl VerterHost block to host_methods.rs                         |
| 44e94f81  | 10   | extract walker + macro-shape member traversal to macro_member_walk.rs   |
| 0373af6f  | 11   | extract registry structural materialization to registry_materialize.rs  |
| 8a8c6f34  | 12   | extract graph-native registry-route + cycle-BFS predicates              |
| add00246  | 13   | extract origin-graph builder to origin_graph.rs                          |
| 0182b0dd  | 14   | extract HostComponentMetaResolver adapter + JSDoc helpers                |
| (next)    | 15   | mark phase 11a complete (this commit)                                    |

## Test redesign rationale (commit 7)

The original `phase_05m_class_b_callers_migrated_through_bridge_helpers`
test fused the shell + dispatch_helpers + (later) field_types siblings
into a synthetic source string and searched for a
[bridge-header, bridge-end-marker] range to identify "outside the bridge
section". That mechanism became fragile under the folder split: re-homing
the §4.10 K1 marker to `materialize/field_types.rs` required extending
the concat list, and adding any new sibling under `meta_resolve/` would
have required hand-extending the fusion order.

The redesigned guard walks every `.rs` file under
`crates/verter_session/src/meta_resolve/` plus the shell `meta_resolve.rs`
and asserts the post-split architectural invariant **per-file**:

1. **Class B engine-method callsite patterns**
   (`.project_type_surface_expr(`, `.project_type_surface_shape(`,
   `.project_prepared_type_surface_expr(`,
   `.project_prepared_type_surface_shape(`) MUST be 0 in every
   meta_resolve sibling EXCEPT `meta_resolve/dispatch_helpers.rs`
   (the single allowed bridge home).
2. **The §5.14.2 bridge section header** MUST be present in
   `meta_resolve/dispatch_helpers.rs`.
3. **The stale `project_type_class_b_via_dispatch` helper aliases** MUST
   NOT appear in any meta_resolve sibling.
4. **`host_manage.rs` Class B engine refs** continue to be 0.

### Discrimination verified

* Temporarily injecting a fake Class B callsite into
  `meta_resolve/scoring.rs` made the test FAIL with the expected
  per-file message. Reverted before commit.
* Temporarily mangling the §5.14.2 bridge section header in
  `meta_resolve/dispatch_helpers.rs` made the test FAIL with the expected
  "bridge section header missing" message. Reverted before commit.
* The walkdir shape is robust under further folder splits — Phase 11a
  commits 8–14 added new siblings without test edits, and the test
  continued to pass against each post-commit tree.

## Public-API preservation verification

Every external caller (`@verter/component-meta`, the LSP, MCP, the
unplugin, `verter_compiler` test fixtures, `verter_tsc`, etc.) reaches
the moved items through the shell's `pub(crate) use sibling::*;` block.
Public types (`ResolvedComponentMetaState`, `SurfaceNodeIdentities`,
`CapturedComponentMetaInputs`, `ResolvedTypeDeclaration`,
`SessionRequestHost`, etc.) re-exported via `pub use` — the only
change is the re-export source path, which is invisible to consumers.

`cargo build --workspace --tests` succeeds against the post-Phase-11a
tree without any external code change.

## §0.6.1 mechanical adjustments (test path updates)

Three lib-internal static-text tests anchored on `meta_resolve.rs`
required literal file-path updates after the architectural moves they
discriminate were applied. The test mechanisms are preserved verbatim;
only the literal paths moved to track the file moves.

| Test                                                                                                                                  | Pre-Phase-11a path             | Post-Phase-11a path                                       |
|---------------------------------------------------------------------------------------------------------------------------------------|--------------------------------|-----------------------------------------------------------|
| `crates/verter_session/tests/origin_graph_audit_contract.rs::gate_text_includes_audit_enabled` (commit 9)                              | `meta_resolve.rs`              | `meta_resolve/host_methods.rs`                             |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs::tests::step6_2_member_route_fast_path_runs_before_eager_materialize` (commit 10) | `meta_resolve.rs`              | `meta_resolve/macro_member_walk.rs`                        |
| `crates/verter_session/src/d_cutover_characterization_tests.rs::phase_05e_commit_6_instantiate_local_generic_ref_callers_migrate_to_dispatch` (commit 12) | `meta_resolve.rs`              | concatenation of `meta_resolve.rs` + `meta_resolve/registry_materialize.rs` + `meta_resolve/dispatch_helpers.rs` |

The discrimination property is preserved in each case: a future commit
that drops the asserted invariant FAILS the test against the post-split
tree exactly as it would have FAILED against the pre-split tree.

## Architectural authorization (§0.6.2 exception)

The user explicitly authorized the redesign of
`phase_05m_class_b_callers_migrated_through_bridge_helpers` (commit 7)
as the only architectural addition for this continuation worker. All
other test path updates (commits 9, 10, 12) are §0.6.1 mechanical
adjustments — the test mechanism is unchanged, only the literal file
path tracks the architectural move.

## Anchor drift log

§11a.0.3 anchor verification was performed at the start of the
continuation work:

* `pub fn resolve_component_meta` — pre-split anchor `meta_resolve.rs:6029`
  → final landing `meta_resolve/host_methods.rs` (within the moved
  `impl VerterHost { ... }` block).
* `pub struct ResolvedComponentMetaState` — pre-split anchor
  `meta_resolve.rs:1246` → final landing `meta_resolve/resolved_state.rs`
  (foundation commit 11a.5).
* `pub(crate) fn resolve_type_declaration` — pre-split anchor
  `meta_resolve.rs:12540` → final landing `meta_resolve/jsdoc_resolve.rs`
  (commit 14).
* `pub(crate) fn extract_route_root_identity_node` — pre-split anchor
  `meta_resolve.rs:10802` → final landing `meta_resolve/graph_predicates.rs`
  (commit 12).
* `pub(crate) fn materialize_component_meta_type_expr_until_stable` —
  pre-split anchor `meta_resolve.rs:2015` → final landing
  `meta_resolve/materialize/field_types.rs` (commit 8).

All anchors landed in the §11a.2 plan-named per-domain siblings without
drift > tolerance.

## Test results

* **Workspace** — `cargo test --workspace --tests --verbose`:
  10,283 passed, 0 failed across 45 test blocks.
  Output captured at `/tmp/p11a-cont-workspace.txt`.
* **Correctness** — `cargo test -p verter_session --test correctness`:
  18 passed, 0 failed (1 ignored — pre-existing).
  Output captured at `/tmp/p11a-cont-correctness.txt`.
* **Redesigned phase_05m guard** — passes against the post-Phase-11a
  tree; discriminates on injected Class B callsites in non-bridge
  files (manually verified).

## Deferred

None. Phase 11a is an atomic-gate phase per §0.3 ATOMIC_GATE_PHASES;
all planned work landed in this continuation. `god_module_size_budget`
guard flip is reserved for Phase 11e per §11.2a.

## Out-of-scope items observed (NOT addressed)

The pre-existing clippy `dead_code` warnings on the verter_session lib
inherited from the foundation commits remain unchanged in count and
identity. The continuation worker introduced ZERO new clippy errors;
the existing warnings track legitimately unused symbols (e.g.,
`dispatch_projected_keyspace`, the never-used `project_type_surface_expr_via_host`
non-threaded variant) that are out of scope per §11a.7 #9 ("Removing
dead code").
