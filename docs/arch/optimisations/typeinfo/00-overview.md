# Typeinfo / component-meta performance optimisation program — overview

**Audience:** the implementation agent (separate machine) landing these optimisations in production.
**Scope:** the shared type-resolution substrate (`verter_session` typeinfo engine) as exercised by
`bench:meta:ui` — the wins apply to every consumer of the shared engine (component-meta, typeinfo graph
requests, LSP hover/completion), not just component-meta.

## Benchmark under optimisation

```bash
pnpm --filter @verter/benchmark bench:meta:ui -- \
  --ui-root=<repo>/.integration-tests/repos/nuxt-ui \
  --backends=verter --scenarios=repo_first_pass --expected=none \
  --output-dir=<out> --hard-timeout-ms=5000000
```

- Corpus: nuxt-ui (v4 branch), 179 `.vue` components under `src/runtime/components`.
- Harness (`packages/benchmark/src/meta-ui-bench.ts` + `meta-ui-query-worker.ts`): one worker process per
  scenario; the compat checker (`@verter/component-meta/compat`, `createCheckerByJson`) is created once,
  **all 179 components are `updateFile`d up-front as session overlays** (benchmark-transformed sources),
  then each component is queried sequentially with `checker.getComponentMeta(abs)` (ONE NAPI call each:
  `NapiMetaSession::get_component_meta` → `ComponentMetaSession::get_component_meta_payload`).
  Per-component latency includes the JS-side compat mapping + `normalizeComponentMetaArtifact`.
- `repo_first_pass` = cold shared host, sequential first-touch of every component.

## Baseline (measurement machine)

- Machine: Apple M3, 8 cores, 24 GB, macOS 26.4.1. Release NAPI build (`opt-level=3, lto=true, codegen-units=1`).
- Branch/commit: `refactor/semantic-db-overhaul` @ `300be6dcd`.
- Steady state: **≈ 19.3–19.6 s** for the 179-component pass (runs: 19 254 / 19 486 / 19 599 ms). Setup ≈ 0.2–0.7 s.
- Distribution (successful components): p50 = 41 ms, p95 = 328 ms, p99 = 1851 ms, max = 2050 ms (`Table.vue`).
- Head-heavy: top-2 components (`Table.vue` 2050 ms, `ChangelogVersions.vue` 1851 ms) ≈ 24 % of measured
  time; top-25 ≈ 62 %.
- 20/179 components fail on this branch with `output materialization error: component-meta output
  materialization failed at props[].type index N` (Button, Carousel, ColorModeSelect, ContentSearch,
  ContextMenuContent, DashboardSearch, DashboardSidebarCollapse, DropdownMenuContent, Editor* (3), Input,
  InputMenu, InputNumber, InputTags, Link, LocaleSelect, prose/A, Separator, EditorToolbar). This is a
  **pre-existing correctness regression unrelated to the perf work**; the A/B protocol requires the failure
  set to stay byte-identical.
- Isolation datum: `Table.vue` alone on a fresh host (same harness) = **6 127 ms** — the full pass pre-warms
  ~3× of its work via shared dependency state. `ChangelogVersions.vue` alone = 2 599 ms.
- The pass is effectively single-threaded: main thread = 95.3 % of process CPU; the `verter-decl-lower-*`
  workers contribute ~0.6 % each; JS-side (V8) work ≈ 2.3 %.

## Profile evidence (CPU-weighted samply over the full pass)

Share of total process CPU (all threads, threadCPUDelta-weighted):

| Frame | Share | Meaning |
|---|---|---|
| `ComponentMetaSession::get_component_meta_payload` | 93.0 % | whole query path |
| `compute_component_meta_state_inner` | 64.7 % | native resolution |
| `ProjectSemanticDispatch::execute_via_cold_build_helper` | 62.3 % | semantic-query cold builds |
| `materialize_prepared_decl_bundle_via_ctx` | 51.5 % | **session-overlay bundle rebuild storm (T1)** |
| `build_prepared_import_canonicalization` | 53.3 % | per-bundle full import-chain canonicalization |
| ├─ `resolve_imported_type_root_with_facts_with_store_view` | 25.3 % | re-export chain walks |
| ├─ `build_named_type_export_route_entry` | 13.1 % (30.7 % total) | route-entry building (17.6 % more under cold closures) |
| └─ `observe_content_pinned_indexed` | 10.2 % | per-import artifact observation |
| `resolve_eval_dependency_canonical` | 42.6 % | **dependency-canonical probing (T2/T3)** |
| ├─ `FileArtifactStore::get_any` (DashMap full iteration) | 14.8 % | **whole-store scan per call (T4)** |
| ├─ `alloc::fmt::format` (candidate strings) | 12.6 % | **eager candidate `format!` (T3)** |
| └─ `FilesystemWorkspace::file_exists` (`stat`+`open`) | 6.3 % | real fs probing (T9) |
| `hash_route_surface` (3 call sites) | ≈ 12 % | **sort+hash per call of immutable surface (T5)** |
| DashMap SipHash (`hash_u64`) | 4.1 % | default hasher cost (T6 — rejected, see below) |
| `glob::Pattern::new` + matching under `owners_for_file` | ≈ 2.0 % | glob recompiled per match (T8) |
| `bump_hit_counter` + `bump_access_tick` | ≈ 1.5 % | per-hit key clone + `Arc<str>` alloc (T7) |

Cross-checks from the audit substrate (deterministic counters, `scripts/benchmark` phase-c driver):
- >1 500 prepared-decl-bundle lookups per component request.
- `prepared_decl_bundle_reject_other` — documented "must stay 0 in steady state" — reaches 720/722
  (Theme), 627 (Calendar), 350 (SelectMenu) on cold single-component runs: see `08-follow-ups.md` (T0).
  In Calendar's single request, `Button.vue`'s bundle cold-materialized **580×**.

## The candidate set

| ID | Doc | Level | One-liner | Isolated expectation |
|---|---|---|---|---|
| T1 | `01-request-scoped-overlay-bundle-memo.md` | macro | request-scoped memo for session-overlay prepared-decl bundles | dominant (up to ~50 % of CPU) |
| T2 | `02-request-scoped-dep-canonical-memo.md` | macro | request-scoped positive memo for `resolve_eval_dependency_canonical` | large; overlaps T1 |
| T3 | `03-lazy-candidate-generation.md` | micro | stop eagerly `format!`ing 13 probe candidates per resolution | ~8–12 % isolated |
| T4 | `04-artifact-store-canonical-index.md` | micro | canonical→base-key index replacing `get_any` whole-store scans | ~10–13 % isolated |
| T7 | `05-entry-embedded-hit-counters.md` | micro | hit/access counters embedded in artifact entries | ~1.5–2 % |
| T5 | `06-route-surface-hash-memo.md` | micro | memoize `hash_route_surface` per immutable `ShallowFileState` | ~8–12 % isolated |
| T8 | `07-glob-precompile.md` | micro | precompile workspace membership globs at snapshot build | ~2 % |
| T0/T9/… | `08-follow-ups.md` | — | reject-churn investigation, fs-exists cache, route-entry table, batch API note | see doc |

Isolated expectations DO NOT ADD UP — T1 absorbs much of T2–T5 call volume; T2 absorbs most T3/T4/T9
traffic on its paths. The measured table below is authoritative.

**Rejected candidates:** blanket FxHash conversion of DashMaps (HashDoS posture: workspace paths and
package names are attacker-influenced when an LSP opens an untrusted repo — keep SipHash for
string-keyed public-facing maps; T4/T7 remove most of the hashing traffic anyway).
`LanguageRegistry::classify_static` micro-tuning (≤1 %, below threshold until after the macro wave).

## Measured results (this machine, 3 interleaved rounds, exact bench command, post-fix protocol)

All builds include the `09-output-materialization-fix` (fixed baseline = base 300be6dcd + fix
45a09a59c; each candidate = its branch + the fix merged; outcome sets identical across all builds:
160 success / 19 known-residual failures). Steady-state = median of 3; run spreads were ±1–2 %.

| build | steady ms (runs) | Δ vs fixed baseline | p50 ms | p95 ms | max ms | peak RSS |
|---|---|---|---|---|---|---|
| fixed-baseline | 20 480 (20390/20480/20956) | — | 42.7 | 345 | 1985 | 720 MB |
| T1 overlay-bundle memo | 9 506 (9489/9506/9589) | **−53.6 %** | 23.3 | 207 | 546 | 672 MB |
| T2 dep-canonical memo | 12 032 (11980/12032/12039) | **−41.2 %** | 27.4 | 213 | 1305 | 698 MB |
| T3 lazy candidates | 16 859 (16828/16859/16884) | −17.7 % | 35.2 | 291 | 1739 | 685 MB |
| T4+T7 artifact-store fastpaths | 16 039 (16028/16039/16121) | −21.7 % | 33.9 | 289 | 1760 | 685 MB |
| T5 route-surface hash memo | 17 595 (17545/17595/17596) | −14.1 % | 36.5 | 305 | 1765 | 680 MB |
| T8 glob precompile | 20 076 (20010/20076/20388) | −2.0 % | 41.8 | 332 | 1977 | 704 MB |
| **combined (fix + all six)** | **4 354 (4344/4354/4431)** | **−78.7 % (4.7×)** | **11.7** | **66.6** | **279** | 724 MB |

Reference points: the BROKEN base (pre-fix, 20 failing components) measured 19.3–19.6 s quiet — the
fix adds ≈ +5 % steady-state of genuinely new work (script-tag-package types + one more component
resolving). The worst component (`Table.vue`) went from 2 050 ms (broken base) / 1 985 ms
(fixed baseline max) to **279 ms** on the combined build; p95 dropped 345 → 67 ms. Peak worker RSS is
flat (±7 %) — the request-scoped memos do not buy speed with memory.

Individual deltas do NOT sum (heavy overlap, as predicted): the combined −78.7 % ≈ T1 stacked with
T2's residual, the store fastpaths, and the small tail items.

**Gate: verified.** The canonical Rust gate (`node scripts/gate.mjs`, both surfaces) ran on the
combined branch: one failure — the `lib_rs_stays_under_line_ceiling` growth guard (858 > 855 from T1's
test-module comment) — fixed by a comment trim (no compiled change), guard re-verified green on both
surfaces; every other suite passed in the full run. Combined branch: `perf/combined-fix-plus-all`
@ `fd21791c0`. Clippy delta vs the fix base: zero new findings; `cargo fmt --all --check` clean.

**Artifact parity: verified.** Every build above (each candidate and the combined branch) produced
byte-identical normalized artifacts to the fixed baseline — 179/179 sha256 matches, including
identical error signatures for the 19 residual failures. The optimisations change WHEN work happens,
never WHAT is resolved.

## Cross-branch reference: `feat/framework-adapters-clean`

Measured under the identical harness/corpus (same machine, quiet, TWO independent runs at the freshly
fetched origin HEAD `1fc8b2323`): 178/179 succeed with `sum_ok = 7.43 s / 7.47 s`, p50 ≈ 30 ms,
p95 ≈ 108 ms, max ≈ 355–368 ms — **and `Theme.vue` hangs indefinitely in BOTH runs** (killed once by
the 5 000 000 ms hard timeout, once by a 120 000 ms cap; under the DEFAULT 5 s timeout the pass reads
as "~10–12 s with one quick failure", which is how this branch earned its "10 s, no issues" reputation). Comparison against
the combined branch on the 159 components that succeed on both: **combined is faster on 148/159,
median per-component ratio 0.41 (~2.4×)** — e.g. `Table.vue` 368 → 152 ms — and combined resolves
`Theme.vue` (the fwclean hang) in ~150–550 ms. The 19 components that succeed on fwclean but fail on
the fixed refactor branch are the strict-honesty classes of `09-output-materialization-fix.md`: the
old branch silently renders those member values as `unknown`; the Stage-10 B6 contract fails them
typed. Net: pre-program, the refactor branch was ~2× slower per component than fwclean with 20
failures; post-program it is ~2.4× FASTER per component, fixes the hang class, and the remaining gap
is the documented ambient-lib + RecursiveRef substrate work.

## Measurement protocol (repeat on the production machine)

**Ordering rule (user-mandated): the output-materialization fix (`09-output-materialization-fix.md`)
lands FIRST; every baseline and every candidate is measured ON TOP of the fix.** The fix changes the
workload (the 20 previously-failing components start doing full resolution), so pre-fix numbers are not
comparable to post-fix numbers.

1. Fixed baseline: base commit + the output-materialization fix; `pnpm install --frozen-lockfile &&
   pnpm run build:native`; run the bench command 3×; record `steadyStateMs`, per-component p50/p95/max,
   outcome counts (must be 179/179 success), peak worker RSS. Also run the ORIGINAL broken base once to
   record the fix's own workload delta (informational, not a regression gate).
2. Artifact parity manifest: run the pass once on the fixed baseline and hash the normalized artifacts
   (`normalizeComponentMetaArtifact` output, stable-stringified, sha256) → the parity manifest.
3. Per candidate: merge the fix into the candidate branch, rebuild, then 3× interleaved runs
   (fixed-baseline, candidate, fixed-baseline, …). Accept when the median steady-state improves beyond
   run-to-run spread (baseline spread here was ±2 %) and ALL 179 artifact hashes match the fixed-baseline
   manifest.
4. Correctness gates: `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`, and the
   canonical Rust gate `node scripts/gate.mjs` (both surfaces) on the combined branch (fix + all winners).
5. Combined branch measured the same way; per-candidate numbers in this doc set are deltas vs the FIXED
   baseline unless explicitly labelled pre-fix.

## Architecture-compliance summary

Every candidate was checked against the repo's CRITICAL rules (and adversarially reviewed by a second
model):
- No new query-time resolution engine; no second read/parse path (single-engine rule intact).
- R17 stays intact: overlay bundles are still NEVER admitted to shared/persistent caches — T1's memo is
  request-scoped and dies with the request's store view.
- ReturnOnly/no-poison: the memos cache only successful results; fenced/partial results are never memoized.
- Read-side authority: memo lifetimes are bounded by the object that owns read validity (the request
  context / request store view) — nothing keys on wall-clock or generation heuristics.
- Cache-population path-independence is unaffected: the memos sit ABOVE the shared caches and only
  short-circuit repeated identical reads within one frozen view.
