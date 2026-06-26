# Verter migration orchestrator

Drives the multi-tier migration described at `<scratch>/verter-debt-and-deferred-fixes-plan.md` (revision 8).

## Roles

- **Orchestrator** — runs from the integration branch checkout (`<repo-root>/`).
  Executes Tier 0 sequentially in-place (single worker), then dispatches Tier 1+ workers
  in worktrees. Maintains state-store at `<scratch>/verter-debt-plan-state.json`.
- **Worker** — runs in a worktree at `<repo_root>-wt/<tier>/`. Implements the assigned
  tier's brief; emits a marker JSON conforming to the §12.4 schema.

## Authority chain (R4)

scheduler -> IndexedReady -> ProjectTypeStore -> SemanticGraph/component-meta -> thin consumers.

## Trunk note

For this migration, `main` = `refactor/semantic-db-overhaul`. The integration branch
`refactor/legacy-to-graph-dispatch-migration` was branched from there, NOT from upstream `main`.

## Artifact layout

- `state-store/` — per-tier progress JSON snapshots (cache).
- `reports/` — per-worker reports cherry-picked from worktree branches post-acceptance.

Source of truth (D71) = phase markers at `crates/verter_session/.phase-markers/` plus
the cherry-picked report files. The state-store is a cache.
