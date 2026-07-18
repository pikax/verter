# Deferred cleanup debt (release-clean review, 2026-07-18)

Cleanup items dispositioned **DEFER** during the release-clean review. None is gate-blocking on
its own; each names its durable owner and the resolution it awaits.

## C3 — drop deprecated `verter_workspace` graph re-exports

Drop the deprecated `verter_workspace` graph re-exports — `ProjectGraph` / `ProjectRank` /
`VfsProjectConfig` / `ProjectGraphBuildResult` at
[`crates/verter_workspace/src/lib.rs:185-201`](../../../crates/verter_workspace/src/lib.rs) (each
already `#[deprecated]`) — and migrate **all** callers to `WorkspaceAccess::configure_resolver`
(`crates/verter_workspace/src/traits.rs`).

- **Breaking + cascades into `verter_lsp`** (external-branch turf; live callers exist in
  `verter_lsp/src` and `verter_bench`) — coordinate with the LSP branch before removal.
- **Not gate-blocking:** the shims are harmless and clippy is green, so this is not a release
  blocker.
- **Disposition:** DEFER, from the release-clean review 2026-07-18. Resolve alongside / after the
  LSP branch lands.
