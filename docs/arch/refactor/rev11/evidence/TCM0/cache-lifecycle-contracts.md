# TCM0 §8 — Cache and lifecycle contracts

Scope: charter item 8. One cache implementation and invalidation law per host process.

## The upstream cache/lifecycle model, as actually implemented (not assumed)

Confirmed live (`package-lock-and-semantic-api.md` §4c) and by source read of the shipped client:

- **Ref-counted, snapshot-scoped source-file cache.** `SourceFileCache` (`dist/api/sourceFileCache.js`)
  keys by `(path, parseOptionsKey, contentHash)`, ref-counted by `(snapshotId, projectId)` pairs.
  `retainForSnapshot` carries unchanged entries forward across a new snapshot; `releaseSnapshot` evicts
  a snapshot's refs. This is a real, working invalidation law — Verter's own cache MUST NOT duplicate it
  with a second, parallel content cache for the same files; it should consume `SourceFile`/`Program`
  handles from this one source, per the Canonical Dependency Cache Rule already governing the rest of
  Verter ("Load and parse each dependency at most once... Cache the parsed state... together").
- **Explicit change signaling, not disk polling.** `updateSnapshot({ fileChanges: { changed, created,
  deleted, invalidateAll } })` — confirmed live that omitting `fileChanges` does NOT pick up on-disk
  edits (`package-lock-and-semantic-api.md` §4c, probe4). Any Verter-side file-watcher integration MUST
  translate VFS change events into explicit `fileChanges` entries; there is no "just re-open and it'll
  notice" fallback.
- **A real, reproduced release-timing gap.** The current snapshot's cache is retained until superseded
  or `api.close()` — disposing a still-latest `Snapshot` does not release its cache
  (`package-lock-and-semantic-api.md` §4c). Verter's own cache-ownership layer must not assume dispose ==
  immediate cache eviction on the TypeScript side; Verter's OWN invalidation law is independent of when
  the upstream client happens to release its own client-side cache.

## The one-cache-per-process rule, applied

Per-process, exactly one cache exists for each of these three concerns — no second parallel cache for
any of them:

1. **TypeScript-side content/AST cache** — owned entirely by the shipped `SourceFileCache`/`Snapshot`
   machinery (§ above). Verter does not build a second AST cache for content TypeScript already caches.
2. **Verter-side semantic cache** — the existing `ProjectTypeStore` (per CLAUDE.md's Cache Architecture
   section) — unchanged by TCM0; this investigation finds no reason for TCM1-TCM4 to touch it, since it
   answers a disjoint question (Verter's own type resolution, never delegated to TypeScript's checker).
3. **Content-mapper transform-output cache** — NEW under TCM1/TCM2, keyed by the typed
   `SourceProjectionMap` (once TCM1 replaces the current string-encoded form —
   `mapping-products-string-surface.md`) plus the same `content_hash`/`parse_env_hash` dimensions
   `FileArtifactStore` already uses, so this does not become a fourth orthogonal cache-key scheme.

## Prepared-artifact key composition (charter's explicit inclusion/exclusion list, verified against the
existing key architecture)

The charter names what MAY and MUST NOT enter a prepared-artifact key. Cross-checked against the R21
env-hash-split rule already in force (CLAUDE.md's Cache Architecture section) — no conflict found:

**May include**: source identity (maps onto the existing `canonical` + `content_hash` dimensions),
framework/language mode (`file_language_id`, already a real `FileArtifactStore` key dimension),
codegen options (`parse_env_hash`, already real), source-unit revisions (already implicit in
`content_hash`), product profile, projection schema identity (NEW — the typed `SourceProjectionMap`'s own
schema version, analogous to `SemanticTypeGraph.schema_version` under the Typeinfo Wire Contract), compiler
ABI (NEW — the exact candidate package pin, since the wire shape is tied to it, per
`package-lock-and-semantic-api.md`).

**Must NOT include** (per charter, and consistent with the existing R21 scoping rule that a cache layer
keys only on dimensions it actually depends on): feature-mask policy (this is `projection-class-
contract.md`'s terminal-policy OUTPUT, computed from the class/relation/region/owner/capability tuple —
including it in the artifact key would make the SAME transformed content produce different cache entries
for different consumers asking for different features, which is exactly the kind of unscoped key the R21
rule already forbids), `projection_policy_id`, UTF-8 vs UTF-16 (an encoding choice at the SERIALIZATION
boundary, not a property of the artifact itself), wire representation, V3 encoding options.

## Invalidation law

One law, stated once: a prepared content-mapper artifact is invalid exactly when its `content_hash` (or
any of the "may include" dimensions above) changes — never when a consumer's feature-mask policy changes,
never when the wire encoding changes. This mirrors the existing R6/R21 cache-identity discipline verbatim,
extended to the one new cache this program introduces rather than inventing a parallel scheme.
