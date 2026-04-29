# Phase 6b Pre-Spawn Audit Report

- HEAD: 3147c02f
- Date: 2026-04-29
- Conducted per Phase 6b sub-plan section 6b.0.4

This report is the single source of truth for section 6b.D2a step 6 and section 6b.D2b step 6.

## 1. Generation audit

Command:

    git grep -n 'bump_content_generation|bump_project_generation|SnapshotGeneration' crates/verter_workspace/ crates/verter_session/

Verbatim output captured to /tmp/p06b-audit-generation.txt (74 hits).

Key findings:
- bump_content_generation() is pub(crate) at crates/verter_workspace/src/engine.rs:168.
  Workspace-engine bump call sites: engine.rs:350, 755, 772; filesystem.rs and memory.rs delegate to engine.
  Matches plan verification context.
- bump_project_generation() at project_type_store.rs:699;
  bump_project_generation_and_evict() at project_type_store.rs:870.
  Already integrated with component_meta_materialize.rs:2227.
- SnapshotGeneration(u64) at workspace_snapshot.rs:27;
  bumped on graph publishes at engine.rs:228.

Verbatim grep output:

```
crates/verter_session/src/component_meta_caches.rs:21://!   [`ProjectTypeStore::bump_project_generation_and_evict`].
crates/verter_session/src/component_meta_materialize.rs:2202:    /// `bump_project_generation_and_evict` is invoked atomically when
crates/verter_session/src/component_meta_materialize.rs:2227:            .bump_project_generation_and_evict();
crates/verter_session/src/component_meta_materialize.rs:2232:            "ref_cycle_db must be wired into bump_project_generation_and_evict — \
crates/verter_session/src/project_global_cache_tests.rs:20://! - E. `ProjectTypeStore::bump_project_generation` is monotonic and
crates/verter_session/src/project_global_cache_tests.rs:183:/// E. `ProjectTypeStore::bump_project_generation` is monotonic — the host
crates/verter_session/src/project_global_cache_tests.rs:190:    let g1 = store.bump_project_generation();
crates/verter_session/src/project_global_cache_tests.rs:191:    let g2 = store.bump_project_generation();
crates/verter_session/src/project_global_cache_tests.rs:1142:fn bump_project_generation_clears_resolved_named_types() {
crates/verter_session/src/project_global_cache_tests.rs:1164:    store.bump_project_generation_and_evict();
crates/verter_session/src/project_type_store.rs:699:    pub fn bump_project_generation(&self) -> u64 {
crates/verter_session/src/project_type_store.rs:870:    pub fn bump_project_generation_and_evict(&self) -> u64 {
crates/verter_session/src/project_type_store.rs:871:        let generation = self.bump_project_generation();
crates/verter_session/src/project_type_store.rs:1053:        let g1 = store.bump_project_generation();
crates/verter_session/src/project_type_store.rs:1055:        let g2 = store.bump_project_generation();
crates/verter_session/src/project_type_store.rs:1140:    /// `bump_project_generation_and_evict` clears every generation-sensitive
crates/verter_session/src/project_type_store.rs:1144:    fn bump_project_generation_and_evict_clears_route_and_result_layers() {
crates/verter_session/src/project_type_store.rs:1190:        let g_after = store.bump_project_generation_and_evict();
crates/verter_workspace/src/engine.rs:23:use crate::workspace_snapshot::{SnapshotGeneration, WorkspaceSnapshot};
crates/verter_workspace/src/engine.rs:37:    snapshot_generation: SnapshotGeneration,
crates/verter_workspace/src/engine.rs:168:    pub(crate) fn bump_content_generation(&self) -> u64 {
crates/verter_workspace/src/engine.rs:228:        let generation = SnapshotGeneration(graph.generation());
crates/verter_workspace/src/engine.rs:350:            self.bump_content_generation();
crates/verter_workspace/src/engine.rs:755:            self.bump_content_generation();
crates/verter_workspace/src/engine.rs:772:            self.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:204:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:464:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:473:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:484:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:536:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:548:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:559:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:574:        self.engine.bump_content_generation();
crates/verter_workspace/src/filesystem.rs:585:        self.engine.bump_content_generation();
crates/verter_workspace/src/lib.rs:139:    ConfiguredOwnerResolution, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
crates/verter_workspace/src/memory.rs:190:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:198:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:381:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:387:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:395:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:506:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:520:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:547:        self.engine.bump_content_generation();
crates/verter_workspace/src/memory.rs:565:        self.engine.bump_content_generation();
crates/verter_workspace/src/published_state_tests.rs:3:use crate::workspace_snapshot::{SnapshotGeneration, WorkspaceSnapshot};
crates/verter_workspace/src/published_state_tests.rs:9:        generation: SnapshotGeneration(gen),
crates/verter_workspace/src/published_state_tests.rs:20:    assert_eq!(root.snapshot.generation, SnapshotGeneration(1));
crates/verter_workspace/src/snapshot_builder.rs:16:    compare_project_precedence, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
crates/verter_workspace/src/snapshot_builder.rs:37:    generation: SnapshotGeneration,
crates/verter_workspace/src/snapshot_builder.rs:157:    generation: SnapshotGeneration,
crates/verter_workspace/src/snapshot_builder_tests.rs:7:    ConfiguredOwnerResolution, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
crates/verter_workspace/src/snapshot_builder_tests.rs:155:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:175:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:193:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:213:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:289:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:319:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:345:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:358:        build_workspace_snapshot_simple(vec![make_fallback("d:/project")], SnapshotGeneration(1));
crates/verter_workspace/src/snapshot_builder_tests.rs:638:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:701:        SnapshotGeneration(1),
crates/verter_workspace/src/snapshot_builder_tests.rs:755:    let result = build_workspace_snapshot(&ws, &[], SnapshotGeneration(1), &vite_opts);
crates/verter_workspace/src/snapshot_builder_tests.rs:793:    let result = build_workspace_snapshot(&ws, &[workspace_str], SnapshotGeneration(1), &vite_opts);
crates/verter_workspace/src/workspace_snapshot.rs:27:pub struct SnapshotGeneration(pub u64);
crates/verter_workspace/src/workspace_snapshot.rs:29:impl SnapshotGeneration {
crates/verter_workspace/src/workspace_snapshot.rs:45:    pub generation: SnapshotGeneration,
crates/verter_workspace/src/workspace_snapshot_tests.rs:67:        generation: SnapshotGeneration(1),
crates/verter_workspace/src/workspace_snapshot_tests.rs:408:// ── SnapshotGeneration ──
crates/verter_workspace/src/workspace_snapshot_tests.rs:412:    let gen = SnapshotGeneration(5);
crates/verter_workspace/src/workspace_snapshot_tests.rs:413:    assert_eq!(gen.next(), SnapshotGeneration(6));
crates/verter_workspace/src/workspace_snapshot_tests.rs:418:    assert_eq!(SnapshotGeneration::default(), SnapshotGeneration(0));
crates/verter_workspace/src/workspace_snapshot_tests.rs:470:        generation: SnapshotGeneration(0),
```

Conclusion: section 6b.0.4 verification context holds at HEAD 3147c02f.

## 2. Wrapper enumeration: lib.rs

Command:

    git grep -nE 'fn (configure_projects|configure_resolver|set_exact_resolutions|set_workspace|clear_compile_cache|close)\b' crates/verter_session/src/lib.rs

Verbatim output:

```
crates/verter_session/src/lib.rs:748:    pub fn set_workspace(&self, workspace: Arc<dyn verter_workspace::WorkspaceAccess>) {
crates/verter_session/src/lib.rs:1056:    pub fn clear_compile_cache(&self) {
crates/verter_session/src/lib.rs:1085:    pub fn close(&self) {
crates/verter_session/src/lib.rs:1132:    pub fn configure_projects(
```

Verified existing host wrappers and current cascades:

- set_workspace (lib.rs:748): set_default_resolve_extensions -> assign workspace -> bump_store_view_epoch. NO project_type_store cascade currently.
- clear_compile_cache (lib.rs:1056): compile_cache.iter_mut clears + resolved_type_cache.clear + eval_env_cache.clear + bump_store_view_epoch. NO route_owned_shallow.clear_all / bump_project_generation_and_evict cascade currently.
- close (lib.rs:1085): notify_delete + scheduler.close_file + alias_to_canonical.clear + last_const_prop_overrides.clear + compile_cache.clear + scheduler.reset/restart_driver + resolved_type_cache.clear + resolver.reset_all + eval_env_cache.clear + provenance.reset + new SemanticDb + bump_store_view_epoch. NO route_owned_shallow / bump_project_generation_and_evict cascade currently.
- configure_projects (lib.rs:1132): ws.configure_resolver + compile_cache.iter_mut import_routes/dependencies clear + resolver.reset_all + resolved_type_cache.clear + eval_env_cache.clear + semantic_invalidate_all + bump_store_view_epoch. NO route_owned_shallow / bump_project_generation_and_evict cascade currently.
- configure_resolver does NOT exist as a host method (only on workspace).
- set_exact_resolutions does NOT exist as a host method (only on workspace; called via host.workspace().set_exact_resolutions at one test site).

Matches section 6b.0.4 verification context.

## 3. workspace() handle uses (multi-line PCRE)

Command:

    rg -n -U 'workspace\(\)\s*(\r?\n\s*)?\.' crates/ packages/

Verbatim output captured to /tmp/p06b-audit-workspace-uses.txt (70 hits). Full output:

```
crates/verter_lsp\src\server_utils.rs:243:            .or_else(|| self.documents.host().workspace().read_file(canonical_id))
crates/verter_lsp\src\server_utils.rs:248:            || self.documents.host().workspace().file_exists(canonical_id)
crates/verter_lsp\src\server_utils.rs:255:        self.documents.host().workspace().realpath(canonical_id)
crates/verter_lsp\src\documents\mod.rs:216:            .workspace()
crates/verter_lsp\src\documents\mod.rs:217:            .notify_upsert(&canonical_id, source.clone());
crates/verter_lsp\src\documents\mod.rs:322:        self.host.workspace().notify_close(&canonical_id);
crates/verter_session\src\cross_file.rs:624:            host.workspace().configure_resolver(vec![IdeProjectConfig {
crates/verter_session\src\component_meta_caches.rs:1353:    /// current `workspace().content_generation()` on:
crates/verter_session\src\component_meta_caches.rs:1471:        let current_gen = host.workspace().content_generation();
crates/verter_session\src\component_meta_caches.rs:1591:    let current_gen = host.workspace().content_generation();
crates/verter_session\tests\component_meta_audit\harness.rs:249:        let key_a = host_a.workspace().project_stable_key(ProjectId(0)).unwrap();
crates/verter_session\tests\component_meta_audit\harness.rs:250:        let key_b = host_b.workspace().project_stable_key(ProjectId(0)).unwrap();
crates/verter_session\tests\component_meta_audit\harness.rs:256:            .workspace()
crates/verter_session\tests\component_meta_audit\harness.rs:257:            .lookup_ambient_symbol(key_a, "Pick")
crates/verter_session\tests\component_meta_audit\harness.rs:260:            .workspace()
crates/verter_session\tests\component_meta_audit\harness.rs:261:            .lookup_ambient_symbol(key_b, "Pick")
crates/verter_session\src\host_manage_tests.rs:2591:        host.workspace().configure_resolver(vec![
crates/verter_session\src\host_manage.rs:519:        let view = self.host.workspace().ambient_libs_view();
crates/verter_session\src\host_manage.rs:5368:            self.workspace().register_audit_sink(sink).ok()
crates/verter_session\src\host_resolve_tests.rs:1184:    host.workspace().configure_resolver(vec![
crates/verter_session\src\lib_tests.rs:1081:    let owners = host.workspace().reverse_deps_for("/src/utils.ts");
crates/verter_session\src\lib_tests.rs:2426:    let resource = host.workspace().resource_snapshot();
crates/verter_session\src\lib_tests.rs:2876:    host.workspace().set_exact_resolutions(
crates/verter_session\src\lib_tests.rs:4095:        let resource = host.workspace().resource_snapshot();
crates/verter_session\src\lib_tests.rs:4244:        let owners = host.workspace().reverse_deps_for("/src/types.ts");
crates/verter_session\src\lib_tests.rs:4265:        let owners = host.workspace().reverse_deps_for("/src/types.ts");
crates/verter_session\src\lib_tests.rs:4285:            .workspace()
crates/verter_session\src\lib_tests.rs:4286:            .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4425:        host.workspace().notify_upsert(
crates/verter_session\src\lib_tests.rs:4430:        host.workspace()
crates/verter_session\src\lib_tests.rs:4431:            .notify_upsert("/lib/aliased.ts", std::sync::Arc::from("export {}"));
crates/verter_session\src\lib_tests.rs:4449:        let owners = host.workspace().reverse_deps_for("/lib/aliased.ts");
crates/verter_session\src\lib_tests.rs:4483:            .workspace()
crates/verter_session\src\lib_tests.rs:4484:            .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4500:                .workspace()
crates/verter_session\src\lib_tests.rs:4501:                .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4507:            .workspace()
crates/verter_session\src\lib_tests.rs:4508:            .reverse_deps_for("/lib/types.ts")
crates/verter_session\src\lib_tests.rs:4529:        host.workspace()
crates/verter_session\src\lib_tests.rs:4530:            .notify_upsert("/src/types.ts", std::sync::Arc::from(types_src));
crates/verter_session\src\lib_tests.rs:4531:        host.workspace()
crates/verter_session\src\lib_tests.rs:4532:            .notify_upsert("/src/shared.ts", std::sync::Arc::from(shared_src));
crates/verter_session\src\lib_tests.rs:4545:        let owners = host.workspace().reverse_deps_for("/src/shared.ts");
crates/verter_session\src\lib_tests.rs:4567:            .workspace()
crates/verter_session\src\lib_tests.rs:4568:            .reverse_deps_for("/src/shared.ts")
crates/verter_session\src\lib_tests.rs:4575:        let owners = host.workspace().reverse_deps_for("/src/shared.ts");
crates/verter_session\src\lib_tests.rs:4596:            .workspace()
crates/verter_session\src\lib_tests.rs:4597:            .reverse_deps_for("/src/shared.ts")
crates/verter_session\src\lib_tests.rs:4608:        let owners = host.workspace().reverse_deps_for("/src/shared.ts");
crates/verter_session\src\lib_tests.rs:4638:            host.workspace()
crates/verter_session\src\lib_tests.rs:4639:                .reverse_deps_for("/src/x.ts")
crates/verter_session\src\lib_tests.rs:4646:        let x_owners = host.workspace().reverse_deps_for("/src/x.ts");
crates/verter_session\src\lib_tests.rs:4655:        let y_owners = host.workspace().reverse_deps_for("/src/y.ts");
crates/verter_session\src\lib_tests.rs:4674:        let owners = host.workspace().reverse_deps_for("/src/lib.d.mts");
crates/verter_session\src\lib_tests.rs:4692:        let owners = host.workspace().reverse_deps_for("/src/Child.vue");
crates/verter_session\src\lib_tests.rs:4710:        let owners = host.workspace().reverse_deps_for("/src/logic.ts");
crates/verter_session\src\lib_tests.rs:4728:        host.workspace()
crates/verter_session\src\lib_tests.rs:4729:            .record_ambient_dependency("/src/Comp.vue", virtual_id);
crates/verter_session\src\lib_tests.rs:4732:            .workspace()
crates/verter_session\src\lib_tests.rs:4733:            .reverse_deps_for(virtual_id)
crates/verter_session\src\lib_tests.rs:4738:        let owners = host.workspace().reverse_deps_for(virtual_id);
crates/verter_session\src\lib_tests.rs:4757:            .workspace()
crates/verter_session\src\lib_tests.rs:4758:            .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4772:                .workspace()
crates/verter_session\src\lib_tests.rs:4773:                .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4795:            .workspace()
crates/verter_session\src\lib_tests.rs:4796:            .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4809:            .workspace()
crates/verter_session\src\lib_tests.rs:4810:            .reverse_deps_for("/src/types.ts")
crates/verter_session\src\lib_tests.rs:4817:        let owners = host.workspace().reverse_deps_for("/src/types.ts");
```

Production-callsite summary (cross-checked against plan section 6b.2.F6.bypass):

- Production reads via .workspace().<method>:
  - server_utils.rs:243 (read_file)
  - server_utils.rs:248 (file_exists)
  - server_utils.rs:255 (realpath)
  - workspace_scanner.rs:594 let-bind (visible only by direct file inspection, not via the rg pattern above)
- Production mutator sites:
  - documents/mod.rs:216-217 (notify_upsert, multi-line)
  - documents/mod.rs:322 (notify_close)
- verter_mcp/src/server.rs:3862, 3873 — re-verified at HEAD 3147c02f: those line numbers are inside #[test] functions in the MCP crate. The MCP crate is downstream of verter_session, so MCP tests would NOT compile against pub(crate) workspace(); they MUST go through host.workspace_read().
- verter_session/tests/component_meta_audit/harness.rs:249, 250, 256-257, 260-261 — integration tests in a downstream test target. MUST go through host.workspace_read().
- verter_session in-crate uses (host_manage.rs:519, 5368; component_meta_caches.rs:1471, 1591) and test-mod uses (cross_file.rs, host_manage_tests.rs, host_resolve_tests.rs, lib_tests.rs many sites) all compile against pub(crate) workspace() after demotion. Mutator sites should reroute to wrappers; read-only test sites can keep using host.workspace().

Reroute table (per section 6b.D2b step 6 + step 7):

| Site | Current | New | Step |
|---|---|---|---|
| verter_lsp/src/documents/mod.rs:216-217 | host.workspace().notify_upsert(canonical, src) | host.notify_upsert(canonical, src) | step 5+6 |
| verter_lsp/src/documents/mod.rs:322 | host.workspace().notify_close(canonical) | host.notify_close(canonical) | step 5+6 |
| verter_lsp/src/server_utils.rs:243 | host.workspace().read_file(...) | host.workspace_read().read_file(...) | step 6 |
| verter_lsp/src/server_utils.rs:248 | host.workspace().file_exists(...) | host.workspace_read().file_exists(...) | step 6 |
| verter_lsp/src/server_utils.rs:255 | host.workspace().realpath(...) | host.workspace_read().realpath(...) | step 6 |
| verter_lsp/src/workspace_scanner.rs:594 | let ws = host_clone.workspace(); ws.as_ref() | let ws = host_clone.workspace_read(); | step 6 |
| verter_session/tests/component_meta_audit/harness.rs:249,250,256-257,260-261 | host.workspace().<read> | host.workspace_read().<read> | step 6 |
| verter_mcp/src/server.rs:3862, 3873 | host.workspace().owner_for_file(...) (in tests) | host.workspace_read().owner_for_file(...) | step 6 |
| crates/verter_session/src/cross_file.rs:624 | host.workspace().configure_resolver(...) | host.configure_projects(...) | step 7 |
| crates/verter_session/src/host_manage_tests.rs:2591 | host.workspace().configure_resolver(...) | host.configure_projects(...) | step 7 |
| crates/verter_session/src/host_resolve_tests.rs:1184 | host.workspace().configure_resolver(...) | host.configure_projects(...) | step 7 |
| crates/verter_session/src/lib_tests.rs:2876 | host.workspace().set_exact_resolutions(...) | host.set_exact_resolutions(...) | step 7 |
| crates/verter_session/src/lib_tests.rs:4425, 4430-4431, 4528-4532 | host.workspace().notify_upsert(...) | host.notify_upsert(...) | step 7 |

Test reads inside verter_session at lib_tests.rs:1081, 2426, 4244, 4265, 4285-4286, 4449, 4483-4484, 4500-4501, 4507-4508, 4545, 4567-4568, 4575, 4596-4597, 4608, 4638-4639, 4646, 4655, 4674, 4692, 4710, 4728-4729, 4732-4733, 4738, 4757-4758, 4772-4773, 4795-4796, 4809-4810, 4817 — reverse_deps_for, resource_snapshot, record_ambient_dependency, etc. — compile against pub(crate) workspace() in the same crate. Default: leave on host.workspace().

## 4. &dyn WorkspaceAccess function signatures

Command:

    git grep -nE 'fn .*&dyn (verter_workspace::)?WorkspaceAccess' crates/ packages/

Verbatim output:

```
crates/verter_workspace/src/config.rs:313:pub fn load_project_membership(ws: &dyn WorkspaceAccess, tsconfig_path: &str) -> ProjectMembership {
crates/verter_workspace/src/config.rs:384:pub fn load_project_references(ws: &dyn WorkspaceAccess, tsconfig_path: &str) -> Vec<String> {
crates/verter_workspace/src/config.rs:404:pub fn has_solution_style_tsconfig(ws: &dyn WorkspaceAccess, workspace_root: &str) -> bool {
crates/verter_workspace/src/config.rs:444:fn is_solution_style_tsconfig(ws: &dyn WorkspaceAccess, tsconfig_path: &str) -> bool {
```

Plus named-function audit (resolve_with_reader, prepare_non_vue_provider_sync) captured to /tmp/p06b-audit-named-fns.txt.

Read-only consumers requiring retype to &dyn WorkspaceRead:
- resolve_with_reader (workspace/resolver.rs:263)
- prepare_non_vue_provider_sync (lsp/server_utils.rs:349)
- collect_resolved_provider_dependencies (lsp/server_utils.rs:378)
- load_project_membership (workspace/config.rs:313)
- load_project_references (workspace/config.rs:384)
- has_solution_style_tsconfig (workspace/config.rs:404)
- is_solution_style_tsconfig (workspace/config.rs:444)

All bodies verified read-only via inspection — they call only read methods (read_file, file_exists, realpath, project_stable_key, etc.).

resolve_with_reader callers (production):

```
crates/verter_lsp/src/server_tests.rs:6455
crates/verter_lsp/src/server_utils.rs:319, 393, 411, 456, 854
crates/verter_workspace/src/engine.rs:465, 588
crates/verter_workspace/src/resolver.rs:644
```

Plus ~40 test sites in resolver_tests.rs.

prepare_non_vue_provider_sync callers:

```
crates/verter_lsp/src/background_init.rs:1185
crates/verter_lsp/src/server.rs:662, 778, 1312
crates/verter_lsp/src/server_tests.rs:1279, 1337
crates/verter_lsp/src/workspace_scanner.rs:595
```

## 5. Decision summary feeding section 6b.D2a / section 6b.D2b

Section 6b.D2a step 6 — extend host wrappers cascade:
- lib.rs::configure_projects: ADD self.project_type_store.bump_project_generation_and_evict() and self.project_type_store.route_owned_shallow.clear_all().
- lib.rs::clear_compile_cache: ADD self.project_type_store.route_owned_shallow.clear_all().
- lib.rs::close: ADD self.project_type_store.route_owned_shallow.clear_all().
- lib.rs::set_workspace: REWRITE to full cascade per plan: bump_project_generation_and_evict, route_owned_shallow.clear_all, resolver.reset_all, resolved_type_cache.lock().clear, eval_env_cache.lock().clear, semantic_invalidate_all, bump_store_view_epoch.

Section 6b.D2b step 4 — retypings:
- resolve_with_reader (workspace/resolver.rs:263)
- prepare_non_vue_provider_sync (lsp/server_utils.rs:349)
- collect_resolved_provider_dependencies (lsp/server_utils.rs:378)
- load_project_membership, load_project_references, has_solution_style_tsconfig, is_solution_style_tsconfig (workspace/config.rs:313, 384, 404, 444)

Section 6b.D2b step 6 — production reroutes: 7 sites enumerated in section 3 above (plus verter_mcp/src/server.rs:3862, 3873 test sites since MCP is a downstream crate).

Section 6b.D2b step 7 — test reroutes: 5 named in-crate test sites + lib_tests.rs::4425/4430-4431/4528-4532 multi-line notify_upsert. Plus tests/component_meta_audit/harness.rs reads.
