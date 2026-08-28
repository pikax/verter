# C1 severed-governance join

Candidate: <FINAL_REVIEW_SHA to be stamped at freeze>

Method (landing-path ruling step 4, a join not a Cartesian product): every relocated unit in
`identity-map.md` (source identity and destination identity) was joined against the repository's
governance attachments in the four named classes, and every hit received exactly one disposition.

- **Origin universe:** every `pub` item removed from a production file between base
  `d1f3d50a9` and the candidate (75 rows), the 3 git-detected whole-file renames, the 2 deletions,
  the resolver split, and the type relocations that landed earlier on the branch.
- **Destination universe:** `crates/verter_semantic/src/resolver_core/*`,
  `crates/verter_semantic/src/facts/{registry,resolution,version}.rs`,
  `crates/verter_workspace/src/{resolver,project_membership,resolution_conversion_tests,…}.rs`,
  `crates/verter_session/src/route_analysis_inputs.rs`.
- **Attachment universe (the four classes):** (1) `CRITICAL_RULE_GUARDS` rows
  (`crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs`); (2) architecture
  guards/scanners under `crates/*/tests/cases/**`, `crates/verter_source_policy_gate/tests/**`,
  `crates/verter_identity/tests/**`, `scripts/*.mjs`; (3) skill/doc pointers in `CLAUDE.md`,
  `AGENTS.md`, `.claude/skills/*/SKILL.md`, `docs/arch/*.md`; (4) allowlists/registries/ledgers
  (`scripts/integration-test-layout-allowlist.json`, `.config/nextest.toml`,
  `performance-gates.toml`, `docs/arch/architecture-lock/ledger/*.toml`, in-guard allowlists).
- **Join key:** every source path, destination path, module path and identifier from the
  identity map (regex over 60 identities). 1,555 raw hits; the frozen command-proof
  transcripts under `docs/arch/architecture-lock/ledger/A*/` (1,157 hits) are immutable
  historical records, not governance attachments, and are dispositioned `none` as a class.

Dispositions: `followed` (attachment already names the destination), `retargeted` (edited in
this branch to name the destination), `retired` (attachment keyed to a deleted identity, removed),
`defect` (correctness/invariant loss repaired), `none` (hit is not a governance attachment keyed
to a moved unit, or the attachment is unaffected).

## Class 1 — `CRITICAL_RULE_GUARDS` rows

| Row / guard | Keyed identity | Disposition | Evidence |
|---|---|---|---|
| `workspace_production_closures_never_cross_upward_except_the_recorded_exception` | `verter_semantic` upward-edge exception (A5-DD1) | **followed** | `crates/verter_identity/tests/cases/workspace_dependency_layers.rs::ratified_upward_exceptions` lists only `verter_diagnostics`; `verter_semantic/Cargo.toml` has no `verter_workspace`/`verter_session`/`verter_scheduler`/`verter_tsgo_api` edge (C1-AC-2). |
| `no_macro_string_heuristics_in_resolver_core` | `verter_session::resolver_core` (unchanged module; the relocated resolver is `verter_semantic::resolver_core`) | **none** | The guard scans the session macro-resolution tree; the relocated module resolver carries no macro surface. Both `resolver_core` modules exist; no row names the moved one. |
| `import_route_writer_guard` | `verter_session::resolver_core` route writers | **none** | Unmoved. |
| `resolver_store_view_returns_store_view_read`, `resolver_store_view_into_owned_view_is_allowlisted` | `ResolverContext::resolver_store_view` (stays in `verter_session`) | **none** | Trait not relocated (addendum GAP 1). |
| every other row | — | **none** | No row names a relocated identity (grep of the registry for every identity in the map: only the four above). |

## Class 2 — architecture guards, scanners, dependency guards

| Attachment | Keyed identity | Disposition | Evidence |
|---|---|---|---|
| `verter_identity/tests/cases/workspace_dependency_layers.rs` layer matrix + exceptions | `verter_semantic` (layer 3) → `verter_audit` (layer 1) edge added this branch | **followed** | Downward edge; the matrix needs no change. `RATIFIED_ROOT_CRATES` unchanged. |
| `verter_session/tests/cases/architecture_guards.rs::guard2_allowlist` (`std::fs::`/`tokio::fs::` text allowlist) | `crates/verter_workspace/src/resolver.rs` | **retired** | The only `std::fs::` text in the base file was the doc comment on `normalize_canonical_id` (`resolver.rs:1936-1937`); the helper moved to `verter_semantic/src/resolver_core/path_utils.rs` without that text and the shrunk `resolver.rs` has zero hits. Row removed (commit `854b67d84`). |
| `scripts/gate.mjs` `OVERSIZE_SOURCE_EXEMPTIONS` | `crates/verter_workspace/src/resolver.rs` | **retired** | 2,172 lines left the file; it is 421 lines, under the 1,500 advisory limit. Row removed. |
| `verter_session/tests/cases/compile-fail/raw_resolver_entry_points_are_private.rs` | `verter_workspace::resolver::ProjectResolver` | **retargeted (pre-existing in branch)** | Fixture now asserts absence of the legacy `ProjectResolver`; the private-helper denial moved to `verter_workspace/tests/compile-fail/resolver_core_helpers_are_private.rs` and the absence gate to `legacy_resolver_surface_is_absent.rs` (commit `90a233763`, the second confirmed severed-governance instance — a fixture that could no longer name what it denied once the `verter_semantic → verter_workspace` edge was cut). |
| `verter_semantic/tests/cases/resolver_observation_compile_fail.rs` + `.config/nextest.toml` override | `cases::resolver_observation_compile_fail` | **followed** | New trybuild rail registered with its slow-timeout override in both `default` and `ci` profiles. |
| `verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs` (`IdeProjectConfig` external non-authority home) | `IdeProjectConfig` | **followed** | Row 7930-7931 names `verter_semantic::resolver_core`. All other `resolver_core` mentions in that file and in `residual_type_expr_body_reader_inventory.rs`, `handle_capable_consumer_guards.rs`, `whole_env_consumer_graph_native_inventory.rs` are `verter_session/src/resolver_core/*` files (unmoved): **none**. |
| `verter_source_policy_gate/tests/cases/tracked_paths_are_portable.rs:376` doc reference | `verter_semantic::resolver_core::path_is_carrier` | **followed** | Names the destination. |
| `architecture_guards.rs::reachability_gc_uses_unified_artifact_name` (`scan_dirs = session, workspace`) | scan scope | **none** | Guard targets the `evict_unreachable_*` name, which did not move. |
| `no_node_modules_substring_outside_workspace_api` | `"/node_modules/"` substring sites | **none** | Scope is `verter_session` decision sites; the relocated `node_modules_resolution.rs` is the resolver's own path algebra, the same status its predecessor `resolver.rs` had. |
| `performance-gates.toml` `workspace.normalize_canonical_id.calls` metric (A6 cell, `absolute_max 11313`) | `normalize_canonical_id` (moved to `verter_semantic::resolver_core::path_utils`) | **defect → repaired** | The move dropped `verter_audit::attribute_n!(NormalizeCanonicalId, …)` and `attribute_n!(CollapsePath, …)` (no `verter_audit` edge in `verter_semantic`), so the gated counter would read 0 and PASS an `absolute_max` gate silently — the class described in `docs/arch/relocation-severs-reachability.md`, on the performance gate itself. Repaired: `verter_semantic` takes the layer-1 `verter_audit` edge and both counters are restored at the moved definitions (commit `f39be3655`). Rail: the counters are compiled only under `verter_audit/attribution` (bench-only), so the default gate cannot observe them; the A6 raw receipt (`a6/`) records the counter at 11,313 on the candidate. Reintroduction of the loss is detected only by the A6 counter dataset, not by a default-gate test — recorded as review-governed residue (see `ac-map.md` A6 note and `.feedback`). |
| `performance-gates.toml` `A6_META_COMPILE_40_COLD_RUST.corpus_fingerprint` | harness blob identity | **defect → repaired (deviation)** | See `a6/receipt.md`. |
| `scripts/gen-typeinfo-ignore-manifest.mjs` (`cross_file_flow_routes_via_resolver_core`) | test id naming `resolver_core` | **none** | Names a `verter_session::resolver_core` route test; unmoved. |

## Class 3 — skill and doc pointers

| Attachment | Keyed identity | Disposition |
|---|---|---|
| `.claude/skills/type-cache-architecture/SKILL.md:997` | `verter_workspace::resolver::IdeProjectConfig` | **retargeted** → `verter_semantic::resolver_core::IdeProjectConfig` (+ the workspace `env_hash` trait impls) |
| `.claude/skills/type-cache-architecture/SKILL.md:1656-1661, 1723` | `crates/verter_workspace/src/fact_registry.rs` | **retargeted** → `crates/verter_semantic/src/facts/registry.rs` as the schema owner |
| `.claude/skills/framework-adapters/SKILL.md:462-464` | `verter_workspace::path_is_carrier` / `carrier_*_provider_path` | **retargeted** → `verter_semantic::resolver_core::*` |
| `docs/arch/fact-based-cache.md:438` | `crates/verter_workspace/src/fact_registry.rs` | **retargeted** |
| `docs/arch/native-flow-return.md:431, 519` | `verter_workspace::fact_registry` schema owner | **retargeted** (the old text also inverted the ownership direction) |
| `docs/arch/tsserver-carrier-membership-design.md:173` | `resolver::provider_id_for_source…` `resolver.rs:245-299` | **retargeted** → `ModuleResolverCore::{…}` in `provider_projection_resolution.rs` |
| `docs/arch/multi-framework-adapters-plan.md:1852, 1935` | `crates/verter_workspace/src/resolver.rs:241/:256` | **retargeted** |
| `docs/arch/path-precise-resolution-currency.md:1244` | `ProjectResolver::nearest_config_for_path` | **retargeted** → `ModuleResolverCore::nearest_config_for_path` |
| `CLAUDE.md:30, 230, 235`, `.claude/skills/{architecture,host-session,component-meta,type-resolution}/SKILL.md` `verter_session::resolver_core` / `ResolverContext` pointers | `verter_session::resolver_core` | **none** — that module and trait did not move (addendum GAP 1); the pointers are accurate |
| `CLAUDE.md:325` `ProjectResolver`/`CarrierRegistry`/`EngineBackend` | `ExternalTsProjectResolver` layer name | **none** — a different type (external-TS project binding), not the module resolver |
| `.claude/skills/type-cache-architecture/SKILL.md:1653` `crates/verter_workspace/src/env_hash.rs` | env-hash impls | **none** — file stays; the five functions are now trait methods on the same file |
| `docs/arch/architecture-lock/ledger/A*/**` transcripts (1,157 hits) | historical output | **none** (immutable records) |
| all other doc hits (`docs/arch/*.md` mentioning `IdeProjectConfig`, `FactKey`, `ResolverContext`, `ConfiguredMembership`, …) | type names re-exported or unmoved | **none** — the identifiers resolve at their documented homes; no path-keyed reference to a moved unit remains (verified by re-running the join regex after the edits: zero hits on `verter_workspace/src/resolver.rs`, `fact_registry`, `verter_workspace::resolver::`, `ProjectResolver::`, `to_project_resolver` outside `docs/arch/refactor/` and the frozen ledger transcripts) |

## Class 4 — allowlists, registries, ledgers

| Attachment | Keyed identity | Disposition |
|---|---|---|
| `scripts/integration-test-layout-allowlist.json` | test binaries | **none** — no C1 file is a standalone `tests/*.rs` binary; all new integration cases are wired through `tests/main.rs` (`verter_semantic/tests/cases/{resolver_core_ownership,resolver_observation_compile_fail}.rs`, `verter_workspace/tests/cases/{legacy_resolver_absence_compile_fail,resolver_core_private_helpers_compile_fail,resolver_stay_ownership}.rs`) |
| `.config/nextest.toml` | `resolver_observation_compile_fail` | **followed** (override added with the fixture) |
| `performance-gates.toml` | see class 2 | **defect → repaired** ×2 |
| `docs/arch/architecture-lock/ledger/authority-registry.toml:751`, `program-state.toml:338` | C1 charter scope text | **none** — program bookkeeping, not code governance; trunk-owned (landing step 1/9) |
| `crates/verter_semantic/src/resolver_core/mod.rs` re-export table, `crates/verter_workspace/src/lib.rs` re-exports | moved identities | **followed** — `verter_workspace` re-exports nothing resolver-shaped except `ide_project_config`; consumers name `verter_semantic::resolver_core` directly (S2-F1: no forwarding alias) |

## Result

- Origin universe: 75 item rows + 6 file-level rows + earlier-phase type rows — every row dispositioned in `identity-map.md`.
- Governance attachments joined: 4 classes, 1,555 raw hits, 30 distinct attachments after collapsing the frozen transcripts; dispositions: followed 7, retargeted 9 (8 doc/skill + 1 pre-existing fixture retarget), retired 2, defect-repaired 2, none for the rest.
- Undispositioned rows: **0**.
- Repairs with structural or discriminating evidence: counter restoration (build-verified by `cargo check -p verter_semantic -p verter_workspace --tests`; runtime value witnessed in the A6 counter receipt), A6 pin (validator PASS + blob/sha derivation recorded), stale allowlist rows (suites green after removal).
