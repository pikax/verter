# 09 — Output-materialization corpus failures: the carrier-gated extraction fix + residual classes

**Kind:** correctness fix (prerequisite for all measurements in this doc set — lands FIRST; every baseline
and candidate number is measured on top of it). **Reference implementation:** branch
`fix/output-materialization-corpus`, commit `45a09a59c` (measurement machine).

## Symptom

20/179 nuxt-ui components failed `getComponentMeta` with the Stage-10 B6 strict typed error
(`crates/verter_session/src/meta_resolve/output.rs`):
`component-meta output materialization failed at <lane> index N: a REQUIRED member-value position has
no representable source …` (`SemanticSourceFailure::UnrepresentableRequiredMemberValue`). Minimal repro:
`Link.vue` `props[0]` (`to?: RouteLocationRaw`).

## Root cause #1 (FIXED): non-carrier files were Vue-script-scanned

Evidence chain (instrumented live, each hop verified):
1. `member.value = DeclRef { vue-router/dist/vue-router.d.ts, RouteLocationRaw }` — vue-router@5's
   entry d.ts is a re-export barrel into `index-BQLwgiyK.d.ts`.
2. The chunk file's shallow inventory was COMPLETELY EMPTY (`exports=0, imports=0, stmts=0` for a
   123 KB file with ~200 exports) → export-route walk Miss → `DeclPlaceholder` → interned
   `Opaque(Miss)` → "unknown-materializing failure carrier" → genuine-miss classification → typed abort.
3. Why empty: the file contains a JSDoc `@example` with a literal `<script setup>…</script>` block.
   `VerterHost::build_eval_script_source_with_extraction`'s artifact-less fallback
   (`crates/verter_session/src/host_manage/eval_program.rs`) ran the forgiving Vue raw `<script>`
   byte-scan on EVERY file, carrier or not — it blanked the whole `.d.ts` to whitespace except the doc
   example, the eval parse panicked (`Unexpected token` at the example's `*` continuation), and the
   header analyzer silently published the DEFAULT-EMPTY inventory.

Corpus blast radius (grep-confirmed): vue-router@5, @regle/core, unhead, @comark/vue, motion-v,
mdast-util-to-hast, micromark/remark all ship script-tag-bearing declaration files — mapping onto the
failing component set (the `acceptedProps[]` failures are child components' `Failed` rows absorbed
through the fallthrough `MergedSourceState::fold`).

## The fix (carrier-gated extraction; typed classification, never raw text)

`build_eval_script_source[_with_extraction]` now takes the `canonical_id`; when NO parse artifact
exists, only a framework-CARRIER canonical (per `verter_language::LanguageRegistry::global()
.classify_static(..)` — the same authority `resolve_route_type_edge` uses) may script-extract; a
non-carrier file passes through unchanged. All carrier behaviors byte-preserved (artifact-driven Vue
extraction; neutral non-Vue blanking; artifact-less forgiving scan for genuine carriers). 10 files,
+196/−8: core in `eval_program.rs`; the canonical is threaded through 7 call sites (`prepared_decl.rs`,
`host_resolve/route_surface.rs`, `framework/script_facts.rs`, `eval_env.rs`,
`resolver_core/component_meta_request_impl.rs`, `component_meta_methods.rs`,
`overlay_materialize.rs`).

Tests (TDD, red with the exact production error → green):
- `non_carrier_dependency_with_script_tag_docs_keeps_member_values_representable` (`meta_tests.rs`) —
  hermetic barrel-shaped end-to-end fixture.
- `build_eval_script_source_never_script_scans_a_non_carrier_file` (`host_manage_tests.rs`) — 4
  non-carrier extensions pass through; `.vue` carrier control keeps extraction.
- The pinned fail-closed rails stay green (`prop_member_value_referencing_nonexistent_type_stays_failed`,
  `present_source_with_interior_unknown_materializing_opaque_fails_output`,
  `component_meta_output_failed_interior_locator_fails_closed_per_source_family`,
  `recoverable_shallow_prop_values_still_complete_as_present`). Full lib suite 4179/0.

Corpus effect: 20 → 19 failures with ZERO new failures (prose/A.vue flips to success), and —
load-bearing beyond the flip — every script-tag-package-typed member corpus-wide now resolves to its
real type instead of collapsing (Link.vue advanced from failing at props[0] to props[7]; its
vue-router-backed members all resolve). This CHANGES resolved content for previously-passing
components, which is why the measurement protocol re-baselines after this fix.

## Residual 19 failures — two substrate-gated classes (NOT fixable at the producer; designs sketched)

**Class (a) — TS standard-lib ambient globals (~17 components).** REQUIRED member values bottoming out
at `BareRef("Element")` / `BareRef("HTMLElement")` (reka-ui/floating-ui `collisionBoundary`/`reference`,
tiptap, nuxt-ui `Modal.vue`/`SelectMenu.vue` `portal`), `IntersectionObserverInit['threshold']`
(embla), `InputHTMLAttributes['enterkeyhint']` (@vue/runtime-dom). Honest "unresolvable residual
carriers" today: the ambient-lib substrate exists (`resolver_core/ambient_resolve.rs`) but its module
doc marks the lazy parse→lowering submission "intentionally deferred to a follow-up";
`resolve_ambient_global` is `#[allow(dead_code)]` with no production caller and `register_ambient_lib`
is test-only. Closure = wiring that substrate: tsconfig `lib` registration → bare-name fallback in the
shared resolver → lazy lib-file lowering through the one dispatch (respect `lib_env_hash` key rules).

**Class (b) — self-referential indexed access.** `Link.vue props[7]` (`href?: NuxtLinkProps['to']`,
inherited by Button.vue): the Instantiate recursion sentinel `Opaque(QueryError::RecursiveRef)` is
baked into the completed surface as the IndexedAccess OBJECT; the walker
(`project_semantic_dispatch/walk.rs`) has a mid-walk re-entry arm for `Opaque(DeclPlaceholder)` but
treats other `Opaque(_)` as terminal Miss. Closure = the sentinel must carry the declaring identity
(a `QueryError::RecursiveRef` schema + interning-identity change shared by relation/materialization
paths) plus a DeclPlaceholder-style re-instantiate arm in the walker. `Separator.vue`'s
`slots[].bindings[].type 0.0` (`InteriorSourceMiss` at `<root>.indexedAccessObject`) is the same
deep-resolution family through ComponentConfig/AppConfig theme chains.

Both classes are engine design decisions (Stage-10 adjacent), deliberately NOT hacked around here —
weakening the fail-closed contract or rendering `unknown` is forbidden by the B6 design
(`docs/arch/stage10-b6-ffi-output-materialization.md`).

## Adjacent defects recorded (feedback file, `[debt]`)

- `resolve_eval_dependency_canonical_with` lacks `.js → .ts/.tsx` companion candidates (hermetically
  reproduced, same failure signature; not corpus-relevant today).
- A panicked eval parse silently publishes a DEFAULT-EMPTY inventory — the masking behavior that hid
  this bug; consider a loud counter/fact for panicked parses.

## Measured result (this machine)

Post-fix full pass (single run, ambient load): 160/179 success, steady p50 ≈ 21.0 s. Authoritative
fixed-baseline numbers live in `00-overview.md`'s measured table.
