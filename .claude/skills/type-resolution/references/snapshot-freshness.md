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
  `record_content_transition`). Every record is made inside the
  resolution-world write that bumped the content generation, at that live
  generation: a record never falls below the previous answer and never runs
  ahead of the live generation. A byte-less transition takes a content
  generation of its own, which is what makes it strictly newer than the key
  its caller was refused under.
- **Subtree** — every canonical under a directory prefix transitioned
  (`delete_dir_all`, a watcher `DirectoryTreeDirty` recovery), because the
  member set cannot be enumerated. Containment is `path_matches_prefix`:
  inclusive, boundary-correct (`/src` never covers `/srcx.ts`), and `/`
  covers everything. Lookup is indexed by ancestor — the canonical and each
  `/`-delimited prefix of it is one map probe — so a lookup costs the
  canonical's depth, not the number of directory events ever recorded.
  Leased canonicals are kept in an ordered index, so a retiring subtree folds
  into exactly the leased canonicals under it (one range seek), never a scan
  of every retained entry.

### Retirement and the floor

Evidence no reader owns retires into one monotone `floor`. A canonical with
no retained evidence answers at least the floor; the floor only rises past
evidence it retires and never past the live content generation. So every
answer is monotone per canonical and never below the canonical's true last
transition: **retirement can make an artifact look stale sooner, never make
a stale artifact fresh.** Unleased evidence retires once
`freshness::DEFAULT_RETIRE_TRIGGER` entries are queued, so the floor moves
rarely. Because every record is made at the live generation, the floor
eventually covers every unleased record — a bulk upsert recorded under one
generation included: a pass that raises the floor to that generation
mid-batch does not turn the batch's later records into future revisions. An
unleased consumer that compares two reads for equality sees a floor rise as a
transition (conservative); a consumer that must discriminate unrelated events
owns its canonical (below).

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
  captured content generation (`HostStoreView::lease_freshness`), taken when
  the request view is built, not when the base was captured. From then on
  retirement cannot raise the floor past that generation; a floor that had
  already passed it is not lowered. The artifacts the view reads are still
  answered from their own evidence, because every stored version holds a
  canonical lease and a leased canonical never answers from the floor.
- Held revision readers — a consumer that captures a canonical's
  `last_content_transition_generation` and later compares it for equality
  takes `VerterHost::lease_content_transition` first and holds it as long as
  the captured revision: the LSP carrier-sync `PendingProviderReady` (from
  the gateway's open-time capture until the receipt is minted) and the
  snapshot provider-sync delivery check. Unrelated retirement then never
  reads as a transition of that canonical.

Resident history is therefore bounded by the retirement trigger plus the
evidence live readers own; with every reader gone, churn drains and map
capacity is released.

### Occupancy and measurement

- Required: `WorkspaceResourceSnapshot::freshness_history`
  (`FreshnessResidency`: exact / leased / subtree / queued entries, backing
  capacities, live view leases, floor), also `FreshnessReaders::residency`.
  The leased count is maintained, not scanned. No production code reads it yet; it is
  the resident-count evidence the retirement tests and resource snapshots
  report.
- Optional, `semantic-observe` only: `FreshnessReaders::observe_snapshot`
  (`FreshnessObserveSnapshot`: index probes by lookups, records and
  retirement folds; retirement passes; retired entries). Compiled out of
  default builds.

### Lock order

The history lock is a leaf: nothing else is acquired under it, and records
run inside the resolution-world mutation like the rest of the per-canonical
bookkeeping. Lease drops may happen under artifact-store map locks. The host
installs a workspace's `FreshnessReaders` into the artifact store under the
same write that publishes the workspace, so overlapping `set_workspace` calls
cannot leave one workspace live with another's history installed.

Lease accounting is checked in every build: releasing a lease whose entry or
record is missing, or whose count is already zero, panics rather than being
treated as a release.
