# Follow-up — typeinfo `evaluate_type_expression` scratch-result cache bypasses fact validation

Owning U-block: **U3** (the cache / fact-model end-state — see
`docs/arch/native-typeinfo-parity-cache-export-session.md` → `U3.CACHE_FACT_MODEL`).
May extend into U4 if the work is sequenced after U3's substrate lands.

Status: **filed, not yet scheduled.** This is the long-term-correct design for a
defect that is **pre-existing on `refactor/semantic-db-overhaul`** and **out of scope**
for U0 (carrier + audit + ledger). It was surfaced — but deliberately NOT fixed — during
the U0 block, after a best-architecture escalation (codex + independent claude architect,
unanimous). The escalation chose Option A: revert the in-block scratch-key / VFS spiral,
keep U0 scoped, and file the correct architecture here.

## Problem

`evaluate_type_expression` memoises its result as `uri -> SemanticNodeId` in a
host-owned long-lived `ScratchCache` and returns the cached `SemanticNodeId`
**directly** on a warm hit, BYPASSING `ProjectSemanticDispatch` and the
`SemanticGraphStore` validated warm path. Validity is decided by a hand-rolled
synthesised-URI cache **key** instead of fact validation. That is the exact
inversion of this branch's cache model:

- The fact-based cache architecture (CLAUDE.md → *Cache Architecture (CRITICAL)*,
  `docs/arch/fact-based-cache.md`) is **read-side authoritative**: a query-identity
  cache value is valid iff its recorded `ReadSetSignature.facts` still validates
  against the live `StoreView`, revalidated on **every** warm hit. Query-identity
  keys are content-free and exclude content/version hashes and
  `fact_dep_signature` (R6 — declaration-keyed families carry the env-bearing
  content-free `ResolvedDeclSlotIdentity` slot); the five env-hash dimensions
  stay split (R21).
- The scratch cache does the opposite: it stuffs every invalidation dimension into
  the **key** and trusts the key.

### Why a hand-rolled "provably complete" URI key is the wrong fix

A complete-by-construction URI key is a contradiction in this architecture:

- To be *provably* complete it must fold in the global monotonic
  `content_generation` counter — which bumps on **every** tracked content edit
  **anywhere** in the workspace, logically invalidating the entire scratch LRU on
  every keystroke. "Complete" only because it is a near-dead cache (a correctness
  fig-leaf, not a cache).
- To be *useful* (path-precise: edit file X invalidates only entries that read X)
  it must reproduce the fact tracer inside a string — i.e. re-implement
  `ReadSetSignature`, badly.

You cannot have both in a key. **Do NOT re-attempt the URI-key completeness
approach** (the U0 spiral chased `mode` → transitive imported-file content →
`project_generation` → the global `content_generation` counter and a VFS
string-prefix exemption, one dimension at a time; each round closed one stale case
while fighting the fact model). It is filed here precisely so it is not retried.

## codexA / codexB findings carried forward (the dimensions the URI key kept missing)

These are the concrete staleness holes the URI-key approach surfaced — they are
symptoms of key-trust, not a key-completion checklist:

- **Missing projection `mode`** — `Identity` vs `Expanded` resolve to different
  nodes, so a same-expression cross-mode hit returned the wrong node (cross-mode
  cache poison).
- **Missing transitive imported-file content** — a key over the scope file's own
  content version did not capture edits to *imported* files the resolution read,
  so an imported-type edit returned a stale node.
- **Missing project / route generation** — a tsconfig / alias / workspace-graph /
  package-target redirect that changed resolution *without* a scope content edit
  did not rotate the key.
- **The direct stale `SemanticNodeId` fast path** — the warm hit returns the node
  id directly without re-entering dispatch, so NONE of the above is caught by the
  validated warm path that every other resolver rides.
- **Rejected: the VFS `verter://typeinfo/` string-prefix `content_generation`
  exemption** — making workspace generation semantics depend on
  `canonical_id.starts_with("verter://typeinfo/")` (`verter_workspace` engine /
  filesystem / memory) keys provenance on string spelling, not provenance. Even
  gated behind host-ingress `ReservedScheme` rejection it is the wrong layer.

## Correct design (Option C, with the Option D refinement)

Do not let `ScratchCache` be a semantic-result oracle. The scratch evaluation
**already resolves through the one shared engine** —
`ProjectSemanticDispatch::execute(Instantiate { .. })` — and that dispatch path
already records and validates `ReadSetSignature` for its own `SemanticGraphStore`
family memo. So:

1. **Key on query identity, not a synthesised URI** —
   `(scope_canonical, mode, expression, extra_imports_syntax)`, content-free
   (R6, in the style of the env-bearing `ResolvedDeclSlotIdentity` slot). No `content_generation`, no `project_generation`, no
   `mode_tag`-into-a-hash, no VFS string in the key.
2. **Decide validity on the value via a recorded `ReadSetSignature.facts`**,
   captured path-precisely over exactly the files/decls the dispatch read (scope
   eval-source, prelude imports, `extra_imports` targets, barrels, transitive
   imports). Revalidate on every warm hit against the live `StoreView`, identical
   to every other resolver cache. An LRU may remain purely as the storage/eviction
   backend — validity ≠ presence.
3. **Two acceptable shapes:**
   - **C:** always re-enter `ProjectSemanticDispatch::execute_read` and let the
     graph memo validate (the scratch LRU becomes a thin `uri -> NodeId`
     convenience over a cache that is *already* fact-validated); **or**
   - a first-class typeinfo scratch-result cache whose entries store
     `node_id + ReadSetSignature + self_root_canonicals` and validate before
     return (the multi-candidate `FamilySlots` pattern).
4. **D refinement — strongly consider DELETING the direct `uri -> node_id` result
   fast path entirely.** If an LRU remains, it bounds scratch artifacts / validated
   result candidates; it must never bypass validation.
5. **Then delete the VFS generation exemption.** Its only reason to exist was that
   the key folded in `content_generation`, so the scratch file's own upsert had to
   not bump it (else the second request self-invalidated). Once the key no longer
   folds in `content_generation`, that self-poisoning failure mode disappears and
   the exemption — plus the `ReservedScheme` ingress rejection, the
   `upsert_internal_synthesis` assert, and the three workspace-layer skips — can all
   go. The cleanest cache is the one that lets the VFS special-case be deleted. If
   internal-synthesis provenance must be distinguished at all, type it as
   provenance, not as a URI-scheme string prefix.

## Acceptance (when this is implemented)

- A selected-leaf / scope / imported-type / route edit flips the
  `evaluate_type_expression` published surface; an unrelated edit keeps the warm
  result (path-precise, not all-or-nothing on the global counter).
- No `content_generation` / `project_generation` / version hash appears in any
  scratch cache **key**.
- No VFS `verter://typeinfo/` string-prefix generation exemption exists.
- The warm path validates a recorded `ReadSetSignature` against the live
  `StoreView` before returning (or re-enters dispatch and lets the graph memo do
  it) — no direct unvalidated `uri -> SemanticNodeId` return.
