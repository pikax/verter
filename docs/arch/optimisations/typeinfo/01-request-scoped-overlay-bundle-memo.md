# T1 — Request-scoped memo for session-overlay prepared-decl bundles

**Level:** macro (the dominant candidate). **Risk:** medium — memo-lifetime/validity reasoning; fully pinned by tests.
**Reference implementation:** branch `perf/t1-overlay-bundle-memo`, commits `01e0f3f2e` + `386846905` (lib.rs-ceiling comment trim) (measurement machine).

## Problem (profiler evidence)

Real IDE/benchmark sessions carry the live component sources as SESSION OVERLAYS (the compat checker
`updateFile`s every component up-front; an LSP session overlays every open/edited file). For an
overlay-bearing canonical, `prepared_decl_bundle_with_context`
(`crates/verter_session/src/host_manage/prepared_decl.rs:401`) must bypass the shared bundle cache —
R17 correctly forbids admitting overlay-derived bundles into shared/persistent caches — and calls
`materialize_prepared_decl_bundle_via_ctx` (line 496) on EVERY call. Each materialisation re-runs
`build_prepared_import_canonicalization` (line 668): a full per-import re-export-chain walk with fact
recording. A single component query performs >1500 bundle lookups (audit counters), so the overlay
owner's bundle is rebuilt hundreds of times per query. Profile: this chain is **53 % of total pass CPU**
(`materialize_prepared_decl_bundle_via_ctx` 51.5 % inclusive; canonicalization split:
`resolve_imported_type_root_with_facts_with_store_view` 25.3 %, `build_named_type_export_route_entry`
13.1 %, `observe_content_pinned_indexed` 10.2 %).

## Design (as adversarially reviewed and landed)

Request-scoped, success-only memo of overlay bundles:

- **Placement: `CanonicalCompletionOverlay`** (`resolver_core/request_store_view.rs`) — the object
  created exactly ONCE per top-level request at the `ViewBoundRequestHost` construction boundary
  (`component_meta_methods.rs:~214`) and `Arc`-threaded into every `SessionResolverContext` the request
  builds. The producer reaches it via a new defaulted trait hook
  `ResolverContext::request_completion_overlay()` (returns `None` everywhere except
  `SessionResolverContext`). Explicit-argument plumbing, never TLS.
  - Rejected: the TLS `RequestContext` (its documented R18 carve-out requires it to "never influence
    resolver" — a memo influences resolution); `SessionResolverContext` itself (per-cold-compute-call,
    multiple per request — forfeits most hits).
- **Key = (raw overlay owner canonical, overlay content hash, `StoreViewCompatToken`).** The compat
  token — the same complete external-validity oracle singleflight lanes coalesce on — closes the one
  real staleness hole: `run_stable_request` retries re-snapshot the base view while SHARING the request
  overlay; an externally-moved world produces a different token → the retry misses and re-materialises.
  The token also folds session-overlay identity (`with_session_overlay` → `compute_compat_token`),
  making cross-view key collisions structurally impossible.
- **Success-only via a `with_cacheability_scope` bracket:** a non-cacheable verdict
  (`note_non_cacheable_read_fan_out`: FencedServe / UnrootableRoute / LeaseMiss — the rail the base
  commit hardens) REFUSES the memo insert, so fenced/unrootable materialisations keep today's per-call
  rebuild AND their per-call negative-mark fan-out (nested scopes do not swallow the fan-out).
  Tombstoned canonicals and `None` results are never memoised.
- Wire-up: in the overlay branch of `prepared_decl_bundle_with_context` (where
  `view.overlay_content_hash_for(canonical)` returns `Some(hash)`), consult the memo before
  `materialize_prepared_decl_bundle_via_ctx`; insert after a successful, cacheable materialisation.
  A provenance counter (`overlay_bundle_memo_hits`) makes hits observable end-to-end.

## Why a memo hit is sound (fact-observation audit — re-verify on a moved base)

- The materialisation performs NO shared-cache admission (R17 comment at the site; route facts are
  returned by value and discarded).
- No positive `FactVersionRef` fan-out happens inside the materialisation; consumers root their
  read-set signatures from the RETURNED bundle (`observed_prepared_type_decl` reads
  `defining_content_hash()`). A memo hit returning the identical `Arc` leaves every enclosing
  producer's read-set identical — never strictly smaller.
- `complete_canonical` promotion is idempotent and already ran under the SAME overlay object when the
  memoised bundle was first built.
- The only load-bearing per-call side effects are the NEGATIVE cacheability marks — handled by the
  success-only bracket above.

## Test contract (all landed on the reference branch, TDD red→green)

(a) same-request second read is `Arc::ptr_eq` memo hit; (b) fresh request sees NEW overlay content;
(b2) memo dies with the request (same content re-materialises next request); (c) base-path canonical
never enters the memo; (d) tombstone never memoised; (e) compat-token retry-safety
(`external_supersession_between_snapshots_misses_memo`); (f) non-cacheable materialisation never
memoised; (g) end-to-end wiring pin through the real `resolve_component_meta_with_view` flow asserting
`overlay_bundle_memo_hits` moves. Keep green: the R17 guards
(`overlay_prepared_decl_no_base_cache_pollution`, 4 tests), `architecture_guards` (204), the full
`verter_session` lib suite.

## Known adjacent debt (pre-existing, unchanged by T1 — logged)

The overlay path's `build_prepared_import_canonicalization` resolves re-export chains through host BASE
state while only validating against the session view — a session-overlaid BARREL's retarget may be
invisible to the walk, and its admissions land in shared `imported_roots` without view discrimination.

## Measured result (this machine)

Smoke (12 components): steady p50 1240 → 800 ms (−35 %); every component improved (e.g. AuthForm
302→180 ms, Avatar 41→19 ms).
Full pass (post-fix protocol, median of 3 interleaved runs): steady 20 480 → **9 506 ms (−53.6 %)**;
p50 42.7 → 23.3 ms; p95 345 → 207 ms; max 1985 → **546 ms**; peak RSS 720 → 672 MB. Outcome parity: identical
160/19 set; artifact hashes identical to the fixed baseline.
