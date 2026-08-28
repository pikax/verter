# C1 twenty-second deviation — F22: item 2's complete owned surface + observation-capability sufficiency confirmed + item 5's migration table substantially expanded

Continuing item 2/5's documentation-completeness work F21 named as the
next step after C1-AC-8. Read `crates/verter_workspace/src/resolver.rs`
in full (2100+ lines, every function) plus the landed observation seam
and re-ran item 5's migration-table inventory against the current tree.
Full consult prompt/output: `/tmp/c1-item2-full-surface-prompt.md` /
`/tmp/c1-item2-full-surface-output.md` (not committed — ephemeral
scratch; this file plus the rewritten sequencing record are the durable
record).

## Verdict

**The three already-landed observation primitives (`path_probe`/
`real_path`/`package_manifest`) are sufficient for the ENTIRE resolver
algorithm — no new `InputKey`, no new `ResolverObservation` method, no
capability blocker.** The 3-field `ModuleResolverCore` storage sketch
(item 2) remains sufficient. Item 5's existing migration table is
accurate where it already has rows, but materially UNDERCOUNTS real
consumers, especially `WorkspaceSnapshot.resolver`'s direct LSP field
borrows and several DTO/helper closures. "For the algorithm and
observation seam: yes [scoped enough]. For item 5 as currently written:
not quite."

## 1. Complete owned function surface (item 2)

Full enumeration in the consult transcript, condensed here by concern
(state classification: **Graph** = `IdeProjectConfig`/compiled graph
only; **Observation** = needs `path_probe`/`real_path`/
`package_manifest`; **Pure** = lexical/JSON computation; **Registry** =
process-global `LanguageRegistry`, not workspace I/O):

- **Entry points**: `ProjectResolver::new` → `ModuleResolverCore::new`
  (Graph). `resolve_with_reader` → `resolve_attempt` returning
  `KernelAttempt<Option<ResolveResult>>` (Graph+Observation).
  `resolve_for_project_with_reader` → `resolve_for_project_attempt`
  (Graph+Observation). `project_exact_result` moves unchanged (pure
  projection). `preferred_specifier_candidates` moves (Graph only,
  pure). Private `preferred_specifier` is DELETED (test-only, no
  production caller). `resolve_tracked`/`resolve_for_project_tracked`
  stay workspace-side as the retry/replay adapter — NOT resolver
  algorithm. New attempt entry points need the attempt's
  `ResolutionBasis`; the workspace adapter owns retries, input loading,
  basis-change restart, output replay, publication.
- **Owner/project selection**: `IdeProjectConfig::{new,matches_file}`,
  `effective_configs_for_path`, `nearest_config_for_path`,
  `project_for_ownership`, `normalized_starts_with`, `compare_projects`,
  `project_rank`, `normalize_canonical_id`, plus the membership closure
  (`ConfiguredMembership::contains`, `StaticMembershipSpec::matches`,
  `CompiledGlob::matches`) — ALL Graph/config-only, zero live I/O. Must
  preserve exactly: sort order; first-match-in-order; duplicate/order
  preservation for references; unresolved references as `None`;
  exact-string tsconfig matching; ambiguous-duplicate-ownership →
  `None`; the depth-256 + active-path cycle guards.
- **Relative/absolute**: `resolve_source_id{,_unowned,_for_project}` +
  path helpers + `probe_path_for_context`/`probe_path`/
  `resolve_existing_path`/`resolve_ts_source_sibling`/
  `resolve_declaration_companion`/`package_follow_is_confirmed`. ONLY
  `resolve_existing_path` and `read_package_manifest_if_present` touch
  live state — candidate classification → `path_probe`; positive hit →
  `real_path`; `package.json` → `path_probe` then `package_manifest`.
  Everything else pure.
- **Workspace aliases**: `sorted_workspace_aliases` + the three alias
  loops (unowned/explicit-project/referenced-project) +
  `resolve_path_mapping_target` — ordering is pure (longest-`find`-first,
  lexical tie-break); actual resolution reaches the 3 observations
  through `resolve_path_mapping_target`.
- **Tsconfig `paths`/`baseUrl`**: `resolve_tsconfig_paths`,
  `resolve_path_mapping_target`, `capture_tsconfig_pattern`,
  `apply_tsconfig_target`, plus `resolve_package_exports`/
  `resolve_manifest_types_entry`/`resolve_legacy_package` when a mapped
  target is itself a package directory. Pattern matching/substitution
  read graph/config only; candidate existence/manifests/realpaths use
  the 3 observations.
- **Project-reference recursion**: `resolve_project_references{,_inner}`,
  `ProjectReferenceTraversalState::seeded_with` — the TRAVERSAL is
  graph-only (never reads directories/manifests merely to follow a
  reference); observations arise only testing the referenced project's
  aliases/paths/baseUrl. In the compiled representation,
  `resolve_project_references_inner` walks `reference_edges[node]`
  instead of searching `self.projects`, preserving order, duplicates,
  unresolved-edge skips, branch-local cycle handling, depth fuse
  identically.
- **`#imports`**: `resolve_package_imports{,_from_dir}`,
  `ancestor_dirs{,_from_dir}`, `match_package_mapping`,
  `resolve_package_target`, `package_conditions`, `resolve_package_path`.
  `ancestor_dirs*` repeatedly calls `parent_dir` — it NEVER checks or
  lists a directory (pure lexical walk). Each ancestor may request one
  manifest; a selected target may request probes/realpaths.
- **node_modules/exports/conditions/legacy**:
  `resolve_node_modules_package{,_from_dir,_from_dirs}`,
  `split_package_specifier`, `resolve_package_exports`,
  `resolve_manifest_types_entry`, `is_declaration_file`,
  `resolve_legacy_package`. `resolve_node_modules_package_from_dirs`
  constructs `{ancestor}/node_modules/{package_name}` per lexical
  ancestor — does NOT enumerate `{ancestor}/node_modules` (no directory
  listing). Manifest fields actually read: `exports`, `imports`, `main`,
  `module`, `types`, `typings`. Condition order is hard-coded per
  request kind (`require`+`default`; `import`+`default`;
  `types`+`import`+`default` ×2). **`package.json#browser` is NOT
  supported at all today** — neither `PackageManifest` nor
  `ResolutionPackageManifest` has the field, `resolve_legacy_package`
  never branches on it. A parity-preserving port documents this
  absence; adding browser support is a SEPARATE semantic change
  requiring its own ruling, not part of this port.
- **Provider graph/carrier projection**: `build_resolve_result`,
  `build_project_resolve_result`, `provider_id_for_source`,
  `path_is_carrier`, `relative_specifier`, `split_path_parts`, plus the
  carrier-helper public API (`provider_ide_id_for_source`,
  `source_id_from_provider_id`, `carrier_ide_provider_path`,
  `carrier_api_provider_path`, `carrier_source_extensions`,
  `strip_carrier_extension`, the two `CARRIER_*` constants) — required
  by the module deletion even though not all are reached from the four
  entry points. Reads the TARGET owner (not importer); carrier
  classification reads the static `LanguageRegistry` (not I/O);
  exact-result projection always reports `ResolutionKind::Bundler`.
- **Preferred-specifier reverse mapping**: `preferred_specifier`
  (DELETE), `preferred_specifier_candidates` (moves, graph-only, pure —
  emits tsconfig-path candidates first then aliases, no dedup),
  `reverse_tsconfig_path`. Keep the EXISTING split: core owns candidate
  generation; Engine keeps round-trip resolution + shortest-selection
  orchestration.
- **Free helper API** (re-exported by the semantic shim, not in the
  four-entry closure, still must be dispositioned):
  `build_known_file_index`, `resolve_known_dependency_id`,
  `resolve_known_dependency_base`, `normalize_known_file_id`, the public
  path helpers (`normalize_canonical_id`/`collapse_path`/`join_paths`/
  `parent_dir`/`is_relative_specifier`/`is_absolute_specifier`) — all
  pure; move to the semantic-owned module or a lower path-utility owner.

## 2. Observation-capability sufficiency — CONFIRMED, no gap

Checked the highest-risk candidates specifically:
`resolve_node_modules_package_from_dirs` (lexical candidate construction
+ manifests/probes only), `ancestor_dirs*` (pure repeated `parent_dir`),
`resolve_package_imports` (one manifest per lexical ancestor, then
target probes), `resolve_project_references` (graph traversal only,
observations only when testing a referenced project's own
aliases/paths), `resolve_existing_path` (exactly `path_probe` then
`real_path` only on File/Directory), `read_package_manifest_if_present`
(`path_probe` + `package_manifest`). The manifest identity split is
already exactly right: `path_probe: /pkg/package.json`,
`package_manifest: /pkg` (directory key) — matches
`InputKey::PackageManifest`/`ResolverObservation::package_manifest`
exactly as landed. Two things confirmed NOT missing capabilities: a
backend MAY internally enumerate a parent directory while answering a
typed probe (loader/driver-side evidence, never a kernel `read_dir`
demand); ancestor recovery is already represented outbound by
`ConsumedResolutionObservationKey::RecoveryScope` (kernel-derived,
driver-replayed) — no `InputKey::DirectoryMembers` needed.

**Witness rules for the full port, made explicit**: every consumed
positive or negative path probe is recorded; `real_path` recorded only
after a positive probe; a demanded manifest records
`PackageManifest { directory }`; higher-priority completed misses remain
in the winning/exhausted witness; `NeedInputs`/`Terminal` discard all
partial `AttemptOutput`; same-basis blocked siblings union through
`priority_frontier`; project-reference recursion merges child output in
traversal order; pure owner selection/JSON mapping/provider
projection/preferred-candidate generation add no observation witness.

## 3. Item 5's migration table — substantially expanded, real gaps found

Existing algorithm-entry-point rows (`resolve_with_reader`,
`resolve_for_project_with_reader`, `resolve_tracked`,
`resolve_for_project_tracked`, private `preferred_specifier`,
`preferred_specifier_candidates`, `project_exact_result`) are ACCURATE
apart from current line numbers and one new consumer (the
`test_support::legacy_resolve_with_reader` bridge, itself scheduled for
deletion at cutover).

**`WorkspaceSnapshot.resolver`'s row materially undercounted LSP
consumers** — the six `Engine` sites remain
(`engine.rs:3298,3330,3633,3790,3857,3921`), but item 5's table MISSED
the LSP's direct field borrows/clones across NINE files: `server_utils.rs`,
`background_drain.rs`, `workspace_scanner.rs`, `sync_coordinator.rs`,
`background_drain_decl_closure.rs`, `server/provider_state.rs`,
`background_drain_owner_loss.rs`, `server/sync_orchestration.rs` (exact
line lists in the consult transcript) — primarily cloning/passing the
resolver into `PublishedResolverSnapshot`, provider-sync helpers,
carrier-sync requests, but still mandatory retyping/repointing sites.

**LSP shim consumers** (`verter_lsp/src/project_resolver.rs`) span
`server/mod.rs`, `server_utils.rs`, `config.rs`, `provider_sync.rs`,
`external_ts/carrier_sync.rs`, `workspace_scanner.rs`,
`carrier_provider_projection.rs`, `server/sync_orchestration.rs` — more
than item 5's original snapshot recorded. Several `server_utils`
resolver parameters are intentionally UNUSED (named `_resolver`) —
delete with their call-site arguments, do not mechanically retype.

**N-API/WASM/session DTO consumers MISSED**: the table's claim that
N-API/WASM helpers "can remain unchanged" is correct only for the two
real analysis functions (`lib.rs:2102,2124` N-API; `lib.rs:640,667`
WASM) — it missed DTO consumers relying on the shim's re-exports:
`verter_napi/src/meta.rs`, `verter_napi/src/lib.rs` (3 sites),
`verter_session/src/component_meta_host.rs`,
`verter_session/src/host_lifecycle.rs`, `verter_session/src/meta.rs`.

**Direct path/carrier-helper consumers span many more files** than the
old generic row implied: `is_relative_specifier`, `collapse_path`,
`normalize_canonical_id`, `path_is_carrier`, `carrier_ide_provider_path`,
`carrier_api_provider_path`, `carrier_source_extensions`,
`strip_carrier_extension` each have their own multi-file consumer lists
(full detail in the transcript) across `verter_session`, `verter_lsp`,
`verter_mcp`, `verter_tsc`.

**Value/type closure needs EXPLICIT rows, not a blanket "moves with the
DTOs"**: the core-facing closure (`ProjectOwnership`,
`ResolveRequestKind`, `ResolvePhase`, `ResolutionContext`,
`ProviderTarget`, `ResolutionKind`, `ResolveRequest`, `ResolveResult`);
the project/config closure (`WorkspaceAlias`, `IdeProjectCompilerOptions`,
`IdeProjectConfig`, `ConfiguredMembership`, `StaticMembershipSpec`,
`CompiledGlob`, `NormalizedGlob` — `ProjectMembership` itself stays
workspace-owned, a config-ingress type not core state, but should STOP
being re-exported through semantic analysis); the ENV-HASH closure
(`IdeProjectConfig`'s four env-hash methods + `project_identity`,
`EnvHashInputs`, `ModuleResolutionMode`, `ConditionSet`) — because
semantic cannot keep depending on workspace after the cutover, these
resolve-domain input values must move WITH the hash methods or into a
lower dependency-neutral owner; `SpecifierKind` needs an explicit
disposition at the same time.

**Tests/bridge/Cargo/guards** (mandatory migration/deletion, not
previously itemized this precisely): `resolver_tests.rs` (now 3,929
lines — move/parameterize around attempt views);
`resolution_witness_contract_tests.rs` (PRESERVE as public-boundary
characterization); `resolution_dual_runner_tests.rs` (DELETE with the
final cutover); `resolver.rs::test_support::legacy_resolve_with_reader`
(DELETE); `verter_semantic`'s `verter_workspace` test-support dev edge
(DELETE when the dual runner disappears); the
`raw_resolver_entry_points_are_private` compile-fail fixture (RETARGET
to the new private attempt boundary); **the A5-DD1 exception row +
`RATIFIED_ROOT_CRATES` + the semantic→workspace canary test in
`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`
(lines 145, 335, 424-430) — ALL must be removed or inverted TOGETHER**,
a genuinely new finding (this specific architecture-guard file was not
previously named anywhere in the sequencing record); `verter_workspace`'s
`pub mod resolver` + resolver re-exports at `lib.rs:103,183-188` (DELETE
— explicitly NO forwarding `ProjectResolver`/`NativeProjectResolver`
alias).

One additional constructor found: `ProjectRegistry::
to_native_project_resolver` (`verter_lsp/src/config.rs:832-838`),
currently only called from `test_utils.rs:111`.

## 4. Is the cutover now scoped enough?

**For the algorithm and observation seam: YES** — no hidden capability
blocker, no prerequisite `InputKey` addition. **For item 5 as currently
written: NOT QUITE** — needs one documentation update incorporating: (1)
the LSP `WorkspaceSnapshot.resolver` sites above; (2) the DTO/membership/
glob/env-hash ownership closure; (3) the canonical public path
(`verter_semantic::resolver_core::ModuleResolverCore`) + explicit policy
that `ProjectResolver`/`NativeProjectResolver` are DELETED, with any
retained workspace re-exports value/helper-only; (4) the explicit
statement that current behavior excludes `package.json#browser` (adding
it would need its own separate ruling, not bundled into this port); (5)
the branch-complete `KernelAttempt`/`AttemptOutput` witness rules
summarized above.

## Explicit instruction, followed

The consult was scoped to documentation/scoping only — explicitly asked
NOT to propose starting the port itself, matching F21's "documentation-
completeness gap, not a structural redesign" framing. No files changed
during the consult. This round's next action: incorporate these findings
into `sequencing.md`'s items 2 and 5, closing the
documentation-completeness gap F21 named.
