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
  edits (`package-lock-and-semantic-api.md` §4c; the control that demonstrates it is `probes/probe2-stale-snapshot.mjs`, NOT probe4 — probe4 is the WITH-`fileChanges` case. Corrected 2026-08-23). Any Verter-side file-watcher integration MUST
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
3. **Content-mapper transform-output cache** — NEW under TCM1/TCM2. **Correction, 2026-08-23**: this
   cache is keyed by the prepared-artifact identity dimensions below (source/content identity,
   framework/language mode, codegen options, source-unit revisions, product profile, projection schema
   identity, compiler ABI) — the SAME `content_hash`/`parse_env_hash`-rooted dimensions
   `FileArtifactStore` already uses — never by the typed `SourceProjectionMap` itself. The
   `SourceProjectionMap` (TCM1's OUTPUT once it replaces the current string-encoded form,
   `mapping-products-string-surface.md`) cannot be the cache KEY for the computation that PRODUCES it —
   that is circular (the key would not exist until after the value it looks up is already computed). The
   original wording of this sentence was wrong as written; the correct key composition is the
   "Prepared-artifact key composition" section immediately below, which this correction defers to rather
   than restates, so this does not become a fourth orthogonal cache-key scheme.

## Prepared-artifact key composition (charter's explicit inclusion/exclusion list, verified against the
existing key architecture)

The charter names what MAY and MUST NOT enter a prepared-artifact key. Cross-checked against the R21
env-hash-split rule already in force (CLAUDE.md's Cache Architecture section) — no conflict found:

**May include**: source identity (maps onto the existing `canonical` + `content_hash` dimensions),
framework/language mode (`file_language_id`, already a real `FileArtifactStore` key dimension),
codegen options (`parse_env_hash`, already real), source-unit revisions (already implicit in
`content_hash`), product profile, projection schema identity (NEW — the typed `SourceProjectionMap`'s own
schema version, analogous to `SemanticTypeGraph.schema_version` under the Typeinfo Wire Contract), compiler
ABI (the existing `build_toolchain_fingerprint` dimension `FileArtifactStore` already carries — **Verter's
own** compiler/codegen build identity, per CLAUDE.md's `FileArtifactStore` key composition).

**Correction, 2026-08-23:** the prior text on this line read compiler ABI as "the exact candidate
[TypeScript] package pin, since the wire shape is tied to it" — that was wrong, and reintroduced the same
class of defect the earlier circularity correction fixed: the TypeScript wire shape is TERMINAL state
(steering: "The terminal policy identity belongs to terminal serialization, not compiler semantics"), so a
TypeScript package/wire identity belongs in the DERIVED-serialization key below, never in the prepared
key. Compiler ABI here means Verter's OWN compiler build identity — changing the certified TypeScript
package (e.g. moving to a later certified build under the same locked contract, per
`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`) must NOT invalidate a prepared artifact
whose Verter-side content, framework mode, codegen options, and schema identity are unchanged.

**Must NOT include** (per charter, and consistent with the existing R21 scoping rule that a cache layer
keys only on dimensions it actually depends on): feature-mask policy (this is `projection-class-
contract.md`'s terminal-policy OUTPUT, computed from the class/relation/region/owner/capability tuple —
including it in the artifact key would make the SAME transformed content produce different cache entries
for different consumers asking for different features, which is exactly the kind of unscoped key the R21
rule already forbids), `projection_policy_id`, UTF-8 vs UTF-16 (an encoding choice at the SERIALIZATION
boundary, not a property of the artifact itself), wire representation, V3 encoding options, and the
certified TypeScript package/build identity (moved to the derived-serialization key below).

## Derived-serialization key composition (was named but not composed; closed 2026-08-23)

The charter names a second key ("A derived serialization key may include...") but this file previously
stopped at the prepared-artifact key alone. Closed here, mirroring the prepared-key section's shape:

**May include**: prepared-artifact identity (the full prepared key above, as one opaque input), terminal
encoder identity (which TCM2 wire encoder produced this view — JSON-RPC `SpanMapSegment` vs. a future
encoder), terminal projection-policy identity (`projection-class-contract.md`'s class × relation × region
× owner × capability → mask tuple, i.e. the terminal policy's OWN identity, not its per-span output),
current wire-contract identity (the certified TypeScript package/protocol version this view targets — this
is where the TypeScript package pin belongs, moved out of the prepared key per the correction above),
requested position encoding (UTF-8 vs UTF-16).

**Must NOT include**: any additional source-content or semantic-computation dependency beyond what the
prepared-artifact identity it derives from already carries — a derived view is a read-time projection
over prepared data, never an independent recomputation. This does not forbid the four terminal-only axes
named above (terminal encoder identity, terminal projection-policy identity, current wire-contract
identity, requested position encoding): none of them re-derives prepared content: they select which
existing read-time PROJECTION of that one prepared artifact this key names, which is the derived key's
entire purpose.

## Invalidation law

Two laws, one per key, stated once each: a prepared content-mapper artifact is invalid exactly when its
`content_hash` (or any of the prepared-key "may include" dimensions, including Verter's own compiler ABI)
changes — never when a consumer's feature-mask policy changes, never when the wire encoding or the
certified TypeScript package changes. A derived-serialization view is invalid exactly when its prepared
artifact changes, OR its own encoder/policy/wire-contract/position-encoding identity changes — recomputing
a derived view NEVER triggers recompilation of the prepared artifact it reads. This mirrors the existing
R6/R21 cache-identity discipline verbatim, extended to the two new caches this program introduces rather
than inventing a parallel scheme.
