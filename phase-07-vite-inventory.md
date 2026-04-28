# Phase 7 — Vite Config Consolidation Inventory

This document classifies every `crate::vite_config::*` reference in the
`verter_lsp` crate per §7.2 of the Phase 7 brief.

## Pre-flight summary

- §7.1.1 — `crate::vite_config::` matches exist in
  `crates/verter_lsp/src/server.rs` (lines 230, 364) and
  `crates/verter_lsp/src/background_init.rs` (lines 34, 58, 65, 81, 84,
  1413). Migration is not done. Proceeding to §7.2.
- §7.1.2 — `verter_workspace::vite_config` exposes
  `find_vite_config`, `analyze_vite_config`, `execute_trusted_vite_config`,
  `discover_vite_aliases`, `get_lkg_or_empty`, `normalize_alias_pair`,
  along with `ViteConfigAnalysis`, `ViteConfigOptions`,
  `ViteConfigTrustInfo`, `TrustedExecResult`. All are re-exported from
  the crate root (`crates/verter_workspace/src/lib.rs` lines 119-123).
- §7.1.3 — `crates/verter_lsp/src/vite_config.rs` exists (1673 lines).
  Inspection confirms it is a near line-for-line duplicate of the
  workspace module: same types, same parser logic, same trusted exec
  script, same LKG cache. It predates the workspace consolidation and
  has not been retired.
- The architecture guard `no_local_vite_helpers_in_lsp` already passes
  un-ignored on `server.rs` and `background_init.rs` (those files do
  not contain `fn read_vite_config / parse_vite_config /
  discover_vite_aliases` definitions). The local definitions live
  inside `crates/verter_lsp/src/vite_config.rs` only.

## Mapping: LSP API ↔ workspace API

| LSP item (in `crate::vite_config`)                  | Workspace equivalent (in `verter_workspace`)                  | Signature delta                                                                                       |
| --------------------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `ViteConfigOptions` (struct)                        | `ViteConfigOptions`                                           | Identical fields/`Default`.                                                                           |
| `ViteConfigAnalysis` (enum)                         | `ViteConfigAnalysis`                                          | Identical variants/fields.                                                                            |
| `ViteConfigTrustInfo` (struct)                      | `ViteConfigTrustInfo`                                         | Identical fields.                                                                                     |
| `TrustedExecResult` (struct)                        | `TrustedExecResult`                                           | Identical fields.                                                                                     |
| `analyze_vite_config(project_root: &Path)`          | `analyze_vite_config(ws: &dyn WorkspaceAccess, root: &str)`   | Workspace API uses `WorkspaceAccess::file_exists` and `read_file` instead of direct disk I/O.         |
| `find_vite_config(project_root: &Path)`             | `find_vite_config(ws: &dyn WorkspaceAccess, root: &str)`      | Same — workspace API takes ws abstraction.                                                            |
| `execute_trusted_vite_config(&Path, &Path, &str)`   | `execute_trusted_vite_config(&Path, &Path, &str)`             | Identical — both spawn Node.js directly. (Disk-bound by nature.)                                     |
| `get_lkg_or_empty(&str)`                            | `get_lkg_or_empty(&str)`                                      | Identical — module-static `LKG_CACHE`.                                                                |
| `discover_vite_aliases(&Path, &str)`                | `discover_vite_aliases(&dyn WorkspaceAccess, &str, &str)`     | Workspace API takes ws abstraction.                                                                   |
| `normalize_alias_pair(...)`                         | `normalize_alias_pair(...)`                                   | Identical signature.                                                                                  |

## Reference inventory

Each `crate::vite_config::*` reference is classified below. Conventions:

- **direct API call** — already calls a workspace-equivalent function
  via the LSP's local re-export.
- **local re-implementation** — LSP-owned helper that duplicates
  workspace logic.
- **LSP-specific concern** — diagnostics, transport, UI-state — keep in
  LSP.

### `crates/verter_lsp/src/lib.rs:17` — `pub mod vite_config;`

- **Classification:** local re-implementation.
- **Rationale:** declares the entire LSP-owned duplicate module. The
  module exists only to mirror `verter_workspace::vite_config`.
  Migration removes this `pub mod` and deletes the file.

### `crates/verter_lsp/src/server.rs:230` — `vite_config_options: tokio::sync::Mutex<crate::vite_config::ViteConfigOptions>`

- **Classification:** local re-implementation (type alias to LSP
  duplicate of the workspace struct).
- **Rationale:** the field stores the same struct shape as
  `verter_workspace::ViteConfigOptions`. Migration replaces the type
  qualifier.

### `crates/verter_lsp/src/server.rs:364` — `crate::vite_config::ViteConfigOptions::default()`

- **Classification:** local re-implementation.
- **Rationale:** constructs the LSP duplicate. Migration replaces with
  `verter_workspace::ViteConfigOptions::default()`.

### `crates/verter_lsp/src/background_init.rs:34` — `vite_opts: crate::vite_config::ViteConfigOptions`

- **Classification:** local re-implementation.
- **Rationale:** field of `BackgroundInitArgs`. Migration replaces
  with `verter_workspace::ViteConfigOptions`.

### `crates/verter_lsp/src/background_init.rs:58, 81, 84` — `Vec<crate::vite_config::ViteConfigTrustInfo>` and constructions

- **Classification:** local re-implementation.
- **Rationale:** the LSP `ViteConfigTrustInfo` is a duplicate of
  `verter_workspace::ViteConfigTrustInfo`. Migration replaces type
  references and the constructor; the `iter().map(...)` shim that
  re-builds an LSP-typed struct from the workspace-typed struct can be
  collapsed once both sides use the same struct.

### `crates/verter_lsp/src/background_init.rs:65` — `vite_opts: &crate::vite_config::ViteConfigOptions`

- **Classification:** local re-implementation.
- **Rationale:** function parameter of `build_published_workspace`.
  The body already constructs a `verter_workspace::ViteConfigOptions`
  from this field manually (lines 70-74). Migration switches the
  parameter type to the workspace type and removes the manual copy.

### `crates/verter_lsp/src/background_init.rs:1413` — `&crate::vite_config::ViteConfigOptions::default()`

- **Classification:** local re-implementation (test).
- **Rationale:** test setup. Migration switches to the workspace type.

### `crates/verter_lsp/src/config.rs:351` — `pub trust_required: Vec<crate::vite_config::ViteConfigTrustInfo>`

- **Classification:** local re-implementation.
- **Rationale:** field of `RegistryBuildResult`. Migration switches
  the type to `verter_workspace::ViteConfigTrustInfo`.

### `crates/verter_lsp/src/config.rs:428` — `vite_opts: &crate::vite_config::ViteConfigOptions`

- **Classification:** local re-implementation.
- **Rationale:** parameter of `ProjectRegistry::from_workspace_roots`.
  Migration switches to the workspace type.

### `crates/verter_lsp/src/config.rs:490` — `use crate::vite_config::{analyze_vite_config, ViteConfigAnalysis};`

- **Classification:** local re-implementation.
- **Rationale:** imports the LSP duplicates. Migration switches to
  the workspace import. The call below at line 491 changes from
  `analyze_vite_config(&root_path)` to
  `analyze_vite_config(&ws, &canonical)` using the FilesystemWorkspace
  already constructed in the same function (config.rs already uses
  `FilesystemWorkspace` for tsconfig discovery on line 445).

### `crates/verter_lsp/src/config.rs:530` — `crate::vite_config::execute_trusted_vite_config(&config_path_buf, &root_path, np)`

- **Classification:** local re-implementation.
- **Rationale:** Migration switches to
  `verter_workspace::execute_trusted_vite_config`. Same signature.

### `crates/verter_lsp/src/config.rs:556` — `crate::vite_config::get_lkg_or_empty(&config_path)`

- **Classification:** local re-implementation.
- **Rationale:** Migration switches to
  `verter_workspace::get_lkg_or_empty`. Same signature.

  Note: `LKG_CACHE` is a module-static `LazyLock<Mutex<HashMap>>`. The
  LSP module had its own copy. After migration the LSP no longer
  populates it — only `verter_workspace::execute_trusted_vite_config`
  does. Since LKG entries are populated only by the trusted-exec path
  and consumed only by `get_lkg_or_empty`, switching both call sites
  to the workspace functions keeps populator and consumer on the same
  static.

### `crates/verter_lsp/src/config.rs:573` — `trust_required.push(crate::vite_config::ViteConfigTrustInfo { ... })`

- **Classification:** local re-implementation.
- **Rationale:** constructs the LSP duplicate. Migration switches to
  `verter_workspace::ViteConfigTrustInfo`.

### `crates/verter_lsp/src/config.rs:1109, 1159, 1222, 1275, 1317` — `&crate::vite_config::ViteConfigOptions { ... }`

- **Classification:** local re-implementation (tests).
- **Rationale:** test fixtures. Migration switches to the workspace
  struct literal.

### `crates/verter_lsp/src/server_tests.rs:7056, 7177, 7283` — `crate::vite_config::ViteConfigOptions::default()`

- **Classification:** local re-implementation (tests).
- **Rationale:** test setup. Migration switches to the workspace
  struct.

### `crates/verter_lsp/src/test_harness.rs:182` — `crate::vite_config::ViteConfigOptions::default()`

- **Classification:** local re-implementation (test harness).
- **Rationale:** Migration switches to the workspace struct.

### `crates/verter_lsp/src/workspace_state.rs:23, 228` — `use crate::vite_config::ViteConfigTrustInfo;`

- **Classification:** local re-implementation.
- **Rationale:** re-exports the LSP duplicate. Migration switches to
  the workspace type.

## Summary counts

- **direct API call:** 0 references.
- **local re-implementation:** 25 references (across 6 files plus the
  module declaration in `lib.rs`, plus the entire 1673-line
  `crates/verter_lsp/src/vite_config.rs` file itself).
- **LSP-specific concern:** 0 references. The LSP `vite_config`
  module contains no transport, diagnostics, or UI-state — it is
  purely duplicated parser/exec/cache logic.

No reference is uncertain. No STOP per §0.6.2 fires.

## Migration plan

Since every reference is a local re-implementation that has an
exact-equivalent workspace API, the migration is one clean cutover:

1. Re-point every `crate::vite_config::Type` to the workspace's
   `verter_workspace::Type`.
2. Re-point each function call:
   - `crate::vite_config::analyze_vite_config(&path)` →
     `verter_workspace::analyze_vite_config(&ws, &path_str)`.
   - `crate::vite_config::execute_trusted_vite_config(...)` →
     `verter_workspace::execute_trusted_vite_config(...)` (same args).
   - `crate::vite_config::get_lkg_or_empty(&str)` →
     `verter_workspace::get_lkg_or_empty(&str)` (same args).
3. Drop `pub mod vite_config;` from `crates/verter_lsp/src/lib.rs`.
4. Delete `crates/verter_lsp/src/vite_config.rs`.
5. Verify `cargo test -p verter_lsp --tests --verbose` passes.
6. Un-ignore `no_local_vite_helpers_in_lsp` and confirm pass.

The migration spans `lib.rs`, `server.rs`, `background_init.rs`,
`config.rs`, `server_tests.rs`, `test_harness.rs`, and
`workspace_state.rs` — all in the `verter_lsp` crate. Per §7.3 the
brief says "one commit per LSP file"; the migration here is a single
cohesive cutover (replace LSP duplicate module with workspace API +
delete the file). I will land it as one commit covering all six call
sites + the deletion. If a per-file split caused intermediate compile
failures (one file removes the module, another still uses it), the
plan-tree would not type-check between commits, which violates the
"each commit must compile" rule. A single migration commit is the
correct grouping.
