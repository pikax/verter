## Reader-Owned Snapshot Freshness

The workspace is the sole content authority, so it owns the per-canonical
freshness rail every retained content-derived artifact is checked against:
`WorkspaceRead::last_content_transition_generation(canonical)`. An artifact
built at content generation `G` is content-fresh only while
`G >= last_content_transition_generation(canonical)`. The artifact-only lane
gates on it (`VerterHost::artifact_only_candidate_is_fresh` against
`IndexedReady.built_at_content_generation`), and a `HostStoreView` reads it
through `StoreViewRoots::artifact_only_whole_hash_at`, clamped to the view's
captured `content_generation`.

### Owner and evidence

`crates/verter_workspace/src/freshness/` owns the history
(`FreshnessHistory`, held by the engine and recorded only at the workspace
mutation chokepoints). It keeps two kinds of evidence:

- **Exact** — one canonical transitioned (overlay write/clear, snapshot
  inject/remove, disk write/copy/delete, an explicit byte-less
  `record_content_transition`). Every record answers strictly newer than the
  previous answer for that canonical.
- **Subtree** — every canonical under a directory prefix transitioned
  (`delete_dir_all`, a watcher `DirectoryTreeDirty` recovery), because the
  member set cannot be enumerated. Containment is `path_matches_prefix`:
  inclusive, boundary-correct (`/src` never covers `/srcx.ts`), and `/`
  covers everything. Lookup is indexed by ancestor — the canonical and each
  `/`-delimited prefix of it is one map probe — so a lookup costs the
  canonical's depth, not the number of directory events ever recorded.

### Retirement and the floor

Evidence no reader owns retires into one monotone `floor`. A canonical with
no retained evidence answers at least the floor; the floor only rises past
evidence it retires and never past the live content generation. So every
answer is monotone per canonical and never below the canonical's true last
transition: **retirement can make an artifact look stale sooner, never make
a stale artifact fresh.** Unleased evidence retires once
`freshness::DEFAULT_RETIRE_TRIGGER` entries are queued, so the floor moves
rarely; an unleased consumer that compares two reads for equality sees a
floor rise as a transition (conservative).

### Readers own their evidence

Obtained through `WorkspaceRead::freshness_readers()` (`FreshnessReaders`):

- `CanonicalFreshnessLease` — held by every stored `FileArtifactStore`
  version (the live entry and any retired version a captured root still
  reaches; `FileArtifactStore::install_freshness_readers` is called by the
  host at construction and on every `set_workspace` swap). A leased canonical
  answers from its own retained entry, never from the floor, so retiring
  unrelated history never makes it stale. A subtree entry that retires first
  folds its generation into every leased canonical under it, so a leased
  artifact a directory event made stale stays stale. The entry is released
  with the last version that holds it.
- `ViewFreshnessLease` — held by each `RequestStoreView` for its base's
  captured content generation (`HostStoreView::lease_freshness`). While a
  request runs the floor cannot pass the generation its artifact-only
  answers are clamped to.

Resident history is therefore bounded by the retirement trigger plus the
evidence live readers own; with every reader gone, churn drains and map
capacity is released.

### Occupancy and measurement

- Required: `WorkspaceResourceSnapshot::freshness_history`
  (`FreshnessResidency`: exact / leased / subtree / queued entries, backing
  capacities, live view leases, floor), also `FreshnessReaders::residency`.
- Optional, `semantic-observe` only: `FreshnessReaders::observe_snapshot`
  (`FreshnessObserveSnapshot`: ancestor probes, retirement passes, retired
  entries). Compiled out of default builds.

### Lock order

The history lock is a leaf: nothing else is acquired under it, and records
run inside the resolution-world mutation like the rest of the per-canonical
bookkeeping. Lease drops may happen under artifact-store map locks.
