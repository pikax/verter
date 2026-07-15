# Follow-up opportunities (investigated, not landed in this program)

## T0 — prepared-decl bundle warm-read reject churn (correctness-adjacent, HIGH value)

**Evidence.** The audit counter `PreparedDeclBundleRejectOther` is documented at
`crates/verter_session/src/host_manage/prepared_decl.rs` (`attribute_prepared_decl_bundle_rejection`)
as "fallthrough; must stay 0 in steady state". It is NOT 0: cold single-component audit runs
(`packages/benchmark/src/phase-c-driver.ts`) show 720/722 (Theme.vue), 627 (Calendar.vue),
350 (SelectMenu.vue). Decomposing Calendar's audit record
(`footprint.materializations`): in ONE request, `PreparedDeclBundle` for `Button.vue` cold-materialized
**580 times**, `@nuxt/schema/dist/index.d.mts` 26×, `Icon.vue` 19× — i.e. the shared bundle cache
(`prepared_decl_bundles`, read via `ValidatedFactCache::get_if_valid_self_rooted_attributed` in
`prepared_decl_bundle_with_store_view`) rejects these bundles on EVERY warm read and rebuilds them,
including the full `build_prepared_import_canonicalization` chain walk each time.

**Diagnosis pointer.** The `RejectOther` arm fires when the FIRST rejected fact belongs to a canonical
DIFFERENT from the bundle's keyed canonical (the classifier only special-cases the bundle's own
`FileWholeHash` self-root and its own `ImportRoute` derived hash). So some cross-file fact recorded in
the bundle's `ReadSetSignature` — a route-chain participant's `FileWholeHash` or derived `Route` hash —
fails `validate_with_self_roots` against the live view persistently. Likely causes to check:
1. a fact recorded against a canonical identity the view never tracks (raw vs normalized id skew,
   virtual/companion ids), so strict validation fails forever;
2. a fact recorded from a non-authoritative source (`get_any`-era artifact hash) that disagrees with
   the completion-overlay/scheduler authority consulted at validation time;
3. an edge-generation (`content_generation`) gate suppressing/refreshing a derived hash on one side only.

**Suggested instrumentation.** In `attribute_prepared_decl_bundle_rejection`, temporarily log the exact
`FactVersionRef` (canonical + fact kind + stored vs live hash) for the `RejectOther` arm under a debug
env var, run `node --import tsx packages/benchmark/src/phase-c-driver.ts --components=Calendar`, and
read which fact class loops. Fix the RECORDING side (record the fact under the identity/authority the
validator will consult) — do not weaken the validator.

**Expected impact.** Beyond CPU (each reject re-runs bundle materialization + import canonicalization),
this poisons cross-component reuse on base-path dependencies (node_modules `.d.ts`, shared `.vue` deps).
After T1 (request-scoped overlay memo) lands, this is the next dominant rebuild source on the base path.

## T9 — workspace `file_exists` generation cache (~3–5 % pre-T2)

`NativeFs::file_exists` → `std::fs::metadata` was 3.0 % of pass CPU (+2.6 % `open` syscalls nearby).
Most callers vanish behind T2's memo; if post-T2 profiles still show it, add a positive+negative
existence cache owned by an immutable VFS-generation snapshot (VFS is the change authority —
invalidate by generation swap, never TTL). Cache ONLY the native-filesystem fallback, never
overlay/store visibility, and preserve the missing-vs-IO-error distinction.

## Route-entry construction (~17.6 % under cold closures)

`build_named_type_export_route_entry` cost outside the T1 chain (cold-closure constructions under
`FnOnce`) is only partially removed by T5 (its `hash_route_surface` component). If post-wave profiles
still show it: consider a per-request-view named-export route TABLE (compute a file's full export-route
surface once per (artifact identity, view) and answer per-name lookups from it) instead of per-name
entry construction. Respect the existing `RouteDb` value-side fact rails; this is a read-shaping change,
not a new cache family. Re-measure `resolve_imported_type_root_with_facts_with_store_view` (25.3 %
baseline) after T1+T2 before investing.

## Batch component-meta API (consumer-level, out of scope for this benchmark)

`getComponentMetaBatch` already exists (NAPI → host batch coordinator → `HostCpuPool` fan-out,
one shared overlay view). The benchmark intentionally measures SEQUENTIAL per-query latency, so this
program did not touch it. For real IDE/CI consumers wanting wall-clock throughput over a corpus, the
compat layer could expose an explicit batch warmup/prefetch built on the existing batch API (explicit
batch semantics only — do not silently group ordinary calls; that would change snapshot boundaries).
Native-side parallelism during ONE query is a separate, harder topic: the pass is 95 % main-thread today.

## Memory notes

Peak worker RSS is recorded in the per-candidate A/B tables. The memo candidates cut allocation volume
substantially (fewer rebuilt bundles/strings); a dedicated clone→move audit (e.g. `Arc`ing the biggest
cloned payloads in `execute_via_cold_build_helper` outputs) should be re-profiled AFTER this wave —
at baseline, `Vec::clone`+`String::clone` ≈ 1.3 % and RawVec growth ≈ 1.8 % of CPU; not the lead story.

## Pre-existing correctness regression (NOT perf)

20/179 nuxt-ui components fail on `refactor/semantic-db-overhaul` @ 300be6dcd with
`output materialization error: component-meta output materialization failed at props[].type index N`
(Button, Carousel, ColorModeSelect, ContentSearch, ContextMenuContent, DashboardSearch,
DashboardSidebarCollapse, DropdownMenuContent, EditorEmojiMenu, EditorMentionMenu, EditorSuggestionMenu,
EditorToolbar, Input, InputMenu, InputNumber, InputTags, Link, LocaleSelect, prose/A, Separator).
The A/B protocol treats these as fixed points (identical error signatures required). Fixing them will
also change the perf profile (Button.vue is a hot dependency — see T0).
