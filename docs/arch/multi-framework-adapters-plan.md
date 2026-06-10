# Framework Adapter Substrate + Svelte Proof — Master Program Plan

Verter goes framework-agnostic over the existing shared substrate — never a parallel pipeline.
**This program's goal is the framework adapter substrate plus the Svelte proof vertical**: land the
substrate (B1–B6) and the Svelte vertical (B8a/B8b/B8c), then the reduced final lift (B12). Nothing
else executes in this program. React and Astro designs are preserved as **deferred,
evidence-gated** follow-up material (see "Deferred Verticals" after §8); the Angular vertical is
**extracted** into its own follow-up program document with its own go/no-go decision
(`docs/arch/angular-adapter-program.md`, D-ac). This document remains the single authoritative plan
for the program. It synthesizes the lead architect's verdict and constraint-relaxation follow-up,
the co-architect's three-seam design, and the relaxed-constraint substrate factoring; every
reconciliation is recorded in the Decisions Log (§3). All file:line references were verified
against the live tree on branch `refactor/semantic-db-overhaul`.

---

## 1. Context

Verter was built for Vue: a Vue compiler + LSP that converts SFCs to valid TSX (IDE type checking)
and optimized render functions, plus typeinfo/component-meta surfaces through ONE shared
type-resolution engine. The owner wants the same contract for other frameworks: each
extension-bearing framework gets its own parser + IDE TSX projection, and every framework gets a
"framework surface" (props/emits/slots analogues) served through the shared resolver.

The ground truth (exploration dossier, verified):

- **The wire is already framework-open and landed**: `FrameworkSurfaceRequest` / `ComponentSelector`
  with open-string `framework_adapter_id` / `FrameworkSurfacePayload` / `FrameworkTag` /
  `FrameworkSurfaceKind` / `GRAPH_OPERATION_FRAMEWORK_SURFACES`
  (`crates/verter_protocol/proto/verter/v1/typeinfo.proto:784-866,973,1085-1101`), with request
  validation (`crates/verter_session/src/typeinfo/request_validation.rs:315`). But there is **no
  semantic executor** for the operation, **no `FrameworkAdapterRegistry`**, and
  `TypeInfoGraphResponse` (typeinfo.proto:691) can only return `SemanticTypeGraph | error` — a
  `FrameworkSurfacePayload` cannot be returned at all today.
- **The adapter "abstraction" is module convention only**:
  `crates/verter_session/src/typeinfo/adapters/` contains exactly `vue/` (free functions +
  `impl VerterHost` methods), no trait, no registry.
- **The generalizable trick**: Vue's public component type is a synthesized ordinary value symbol
  (`crates/verter_session/src/resolver_core/vue_default_synth.rs` — a `default` class whose
  construct signature returns `{$props,$emit,$slots}`), resolved through the single shared
  `Instantiate { canonical, "default", [] }` identity. Framework semantics encoded as ordinary TS
  shapes keep the engine framework-blind.
- **Language routing is funnel-shaped but duplicated**: four binary `FileKind {VueSfc, NonSfc}`
  enums (`crates/verter_session/src/types.rs:17`, `crates/verter_scheduler/src/source_loader.rs:40`,
  `crates/verter_scheduler/src/node.rs:173`, `crates/verter_workspace/src/types.rs:6`), a 1:1
  mapping shim (`crates/verter_session/src/host_construction.rs:89-94`), a fifth kind enum
  (`crates/verter_session/src/resolver_core/export_graph.rs:6` `ExportGraphFileKind`), and an FFI
  string map that silently defaults a missing kind to `"vue"`
  (`crates/verter_ffi/src/convert/input.rs:164` — `input.unwrap_or("vue")`).
- **`IndexedReady` carries two Vue-typed fields**
  (`crates/verter_session/src/project_type_store.rs:158,168`):
  `cached_parse: Option<Arc<ParsedSfc>>` and `external_type_analysis` typed under the Vue-named
  module path `verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource` (actual
  owner: `crates/verter_parser/src/utils/oxc/vue/script/resolve_type/`). **Verified additional
  cutover site**: `RouteOwnedShallowEntry` (project_type_store.rs:~532) carries the SAME two fields.
- **Owner constraint relaxation (binding)**: the frozen set is ONLY (a) Vue compiler paths in
  `verter_parser`/`verter_compiler` (Vue parse/codegen **behavior**; mechanical re-export line
  updates are allowed and must be flagged), and (b) typeinfo core (adapters add-only under the
  closed wire rules). `verter_session`, `verter_semantic`, scheduler/workspace, FFI, and TS packages
  may be refactored where best architecture demands. Consequently (lead follow-up): framework-
  neutral script analysis is **physically rehomed now** (FQ1), and `IndexedReady`'s parse payload is
  generalized to an open `FrameworkParseArtifact` wrapper (FQ2).

The program shape (re-scoped, D-ab): **substrate first** (wire completion, language routing,
neutral script analysis, parse-artifact generalization, adapter registry + executor, compiler
scaffold — the substrate proof is B5's Vue framework-surface round-trip parity), then **exactly
one non-Vue vertical: Svelte** (B8a/B8b/B8c, the flagship proof), then the reduced final lift
(B12). React (B7) and Astro (B9) are DEFERRED — their full designs are preserved in the
"Deferred Verticals" section and reopen only on an explicit reassessment with evidence from the
landed seams after the Svelte proof. Angular (former B10/B11) is EXTRACTED into
`docs/arch/angular-adapter-program.md` — a standalone follow-up program with its own go/no-go
decision (D-ac).

---

## 2. Program-Level Invariants (hard; violating any is a STOP)

1. **Exactly ONE type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute`
   → `SemanticGraphStore`, five modes. Every framework adapter is
   `shared_resolve(one type) + thin normalise`. Adapters are NEVER handed `ProjectSemanticDispatch`
   or raw source — and "raw source" includes the host artifact state that carries it:
   `IndexedReady` exposes `raw_source`/`eval_source` (project_type_store.rs:147-160), so
   `ensure_indexed_ready` and every raw/eval/content-snapshot surface are executor/session-private
   and never on the adapter ctx (D-am). Adapters get only the capability-scoped CLOSED-surface
   `FrameworkAdapterCtx` (§B5, op set enumerated by D-am as revised by D-as: carrier metadata +
   validated facts ONLY — no resolve method exists on any adapter-visible ctx; adapters express
   resolution demand as declarative `PlannedDemand` data and the EXECUTOR resolves it through its
   private resolve surface). Any per-framework resolver, per-surface
   walker, or re-parse-and-resolve is a rule violation to delete.
2. **Vue compiler parse/codegen behavior untouched.** No edits to Vue parser/codegen semantics in
   `verter_parser`/`verter_compiler`. Mechanical re-export line updates (e.g.
   `crates/verter_compiler/src/lib.rs:10-17`) are allowed and explicitly flagged for review. Vue
   dispatch rehousing in `verter_session` (host_executor, IndexedReady typing) is in scope and
   pinned by byte-identity characterization suites.
3. **Typeinfo core add-only under the closed wire rules.** Closed-enum discipline, field numbers
   never reused, additive audit, validation-before-execution, schema-version gates, byte-pinned TS
   bindings. This program performs exactly ONE wire block (§B1).
4. **Runtime codegen for non-Vue frameworks is OUT of scope.** New framework compilers target
   `CompileTarget::TSX` (IDE) + `TEMPLATE_DATA` (analysis) only. Official Svelte/Astro/Angular
   compilers remain the runtime authority; unsupported `CompileTarget` bits return typed
   `CompileUnsupported` diagnostics, never silent empties.
5. **CodeTransform is the sole mutation mechanism** for all generated output (sourcemap integrity).
6. **Typed-IR-only.** No text sniffing, no identifier-suffix classification (`ends_with("Props")`
   banned), no synthesize-then-reparse, structural detection via resolved package symbols (e.g.
   `@angular/core` imports, `svelte`'s `Snippet`), framework-specified contracts (Astro's
   `interface Props`) read structurally from the carrier's own scope.
7. **Hermetic vendored fixtures** in all non-gated tests; third-party repos only behind the
   `external-corpus` feature. Official-toolchain oracles are feature-gated comparison benches.
8. **Every new CRITICAL rule lands with a registered guard** in `CRITICAL_RULE_GUARDS`
   (`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`) in the same change.
9. **No phase/temporal vocabulary** in production code or final commit messages; landed code reads
   as final-state.
10. **Cache rules bind every new adapter cache**: content-addressed or query-identity keys per the
    R1–R31 split, FULLY STRUCTURAL key components (a fixed-width digest used AS a key component
    is forbidden — lossy inclusion is not inclusion), `ReadSetSignature.facts` validation, typed
    `SignatureAdmission`, `ReturnOnly` never publishes; the validation pattern proven by
    `VueShallowMetadataStore` (`crates/verter_session/src/typeinfo/adapters/vue/store.rs`) is the
    template for the ONE generic `FrameworkSurfaceDtoStore` substrate (the store itself migrates
    onto it in B5, D-p/D-y — no dual same-role cache survives).
11. **Shallow-by-default / shallow file processing invariants** apply verbatim to every framework:
    one parse + one shallow pass per content hash; framework facts extracted during that ONE pass
    (no rescan); surfaces published shallow unless the consumer walks the path.
12. **TDD throughout**; pre-existing semantic/typeinfo bugs surfaced by new frameworks land as
    known-bug ledger rows (§5), never as core changes inside this program.

---

## 3. Decisions Log (reconciliations)

| # | Decision | Rationale |
|---|---|---|
| **D-a** | **Routing authority = new leaf crate `crates/verter_language`** owning the lead architect's open descriptor: `FileLanguage { Script { source_type }, Framework { adapter_id: FrameworkAdapterId, language_id: LanguageId }, FrameworkTemplate { adapter_id, owner_hint } }` + `LanguageRegistry` (extension table, `classify_static(path)`, `carrier_extensions()`; project-gated rows resolve at the host level per D-r). `FrameworkAdapterId` is an interned `Arc<str>` (open set). | Lead's descriptor shape is authoritative and consistent with FQ2's open `FrameworkParseArtifact.adapter_id`; `FrameworkTemplate` is required for Angular external `.html`. Crate name `verter_language` (co-architect) over `verter_carrier` (rev2) because the crate classifies ALL files (scripts included), not only framework carriers. REJECTED: co-architect's closed `CarrierKind` enum (contradicts the open `framework_adapter_id` wire design — every future framework would need a central enum edit); rev2's `verter_carrier` name. Hot-path note: per-file classification is an interned-id table lookup, no `dyn` in the per-file loop; `canary_warm_hit_zero_alloc` + `perf_bounds/` gate it. |
| **D-b** | **ONE wire block (B1), verified against the live proto** (revised by D-aa): (i) `TypeInfoGraphResponse.kind` gains additive arm `FrameworkSurfacePayload framework_surface = 3` (verified: live oneof has only `graph = 1`, `error = 2` — the landed `FrameworkSurfacePayload` is unreturnable today); (ii) ~~`FrameworkTag` gains `FRAMEWORK_TAG_ASTRO = 6`, `FRAMEWORK_TAG_ANGULAR = 7`~~ — DROPPED per D-aa: no new `FrameworkTag` values land in this program; each tag lands with its own vertical; B1 instead adds the tag-semantics doc comment to the proto (D-aa); (iii) `TYPEINFO_GRAPH_SCHEMA_VERSION` 2→3 (`crates/verter_protocol/src/typeinfo/graph.rs:254`), `MIN` stays 2, `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS = &[2, 3]` (`request_validation.rs:70,86`); (iv) TS bindings regenerated byte-pinned; (v) additive audit coverage for the operation. No other wire change for the whole program. | Both architects' wire requirements are real and complementary; bundling them into one early block pays the schema-bump cost once. `FrameworkSurfaceKind` needs NO new variants — the in-scope Svelte surfaces (and the deferred/extracted designs) map onto the existing six kinds (§9). |
| **D-c** | **Merged ordered block list** (execution scope revised by D-ab; original numbering retained for traceability): EXECUTION SCOPE = B1 wire → B2 routing → B3 neutral script rehoming (FQ1, now in-scope) → B4 `FrameworkParseArtifact` (FQ2) → B5 registry + executor → B6 compiler scaffold → B8a/b/c Svelte → B12 final lift (reduced). Critical path: **B2 → B3 → B4 → {B5, B6} → B8a → B8b/B8c** (B8a requires BOTH B5 and B6 — corrected in fix round 1). B7 (React) and B9 (Astro) are DEFERRED (evidence-gated, D-ab); B10/B11 (Angular) are EXTRACTED (D-ac). | Codex's 10 blocks × co-architect's B1–B11 × rev2's 6 stages merge cleanly once FQ1 rehoming is promoted to its own early block; the substrate proof is B5's Vue round-trip parity (D-t); the Svelte vertical is the single non-Vue proof of this program (owner re-scope). |
| **D-d** | **Macro-recognition generalization (adapter-supplied macro table, rev2 D6) is OUT of this program** — Vue-alignment future work (§11). New frameworks extract surface facts via the one `ScriptFactProvider` seam (D-o) in the ONE shallow pass; their public types are ordinary synthesized TS shapes. | Lead's verdict: `ReductionDemand::MacroObjectSurface` and the Vue macro analyzer stay Vue-only now; "new frameworks must encode their surface as ordinary synthesized TS types or use normal published projection". Svelte runes / Astro `Props` / Angular decorators are not Vue-style call-macros; generalizing `AnalyzedMacroKind` serves no v1 need. REJECTED (recorded): rev2 D6. |
| **D-e** | **Integration branch `feat/framework-adapters` cut from `refactor/semantic-db-overhaul`** (verified: `main..HEAD` = 2355 commits; merge-base = main's tip, branch strictly ahead). Each block lands as ONE squashed conventional commit on the integration branch after the green gate + dual review. When `refactor/semantic-db-overhaul` lands on `main`, convert to codex's short-stacked-branches-on-main model and rebase. **FLAGGED for orchestrator decision** (branch naming + merge timing are the orchestrator's call). | Branching from `main` is not viable: the program builds directly on the overhaul's substrate (typeinfo adapters dir, `SemanticGraphStore` family memo, request validation). Co-architect's branch point + codex's post-landing stacked model reconciled by sequencing them. |
| **D-f** | **Adapter trait = lead's two-phase shape**: `FrameworkSurfaceAdapter { descriptor(), plan_surfaces(ctx, selector, requested) -> FrameworkSurfacePlan, normalize(ctx, resolved) -> FrameworkSurfaceDtoBundle }`, capability-scoped `FrameworkAdapterCtx`. Registry/substrate home refined by D-q: registry + descriptor + ctx + synth substrate at `verter_session/src/framework/`; per-framework surface-adapter IMPLS stay at `typeinfo/adapters/<framework>/`. | The plan/resolve/normalize split makes the EXECUTOR own resolution (adapter plans carriers, executor resolves each through the shared engine, adapter normalizes) — `shared_resolve + normalise` enforced by construction, not just by guard. REJECTED (recorded): co-architect's single `resolve_surface_kind` method (the co-architect's `verter_session/src/framework/` module home is partially ADOPTED via D-q for the registry substrate — the original rejection conflated the trait/adapter home with the registry home; the `FrameworkResolveCx` capability-scoping idea is absorbed into `FrameworkAdapterCtx`). `FrameworkSurfacePlan`'s type contract — the closed `PlannedDemand` vocabulary mapping 1:1 onto the EXECUTOR's private resolve operations — is pinned by D-ai (as revised by D-as); the adapter-visible ctx surface is CLOSED and enumerated by D-am as revised by D-as (carrier metadata + validated facts ONLY — no resolve method, no `ensure_indexed_ready`, no raw/eval source). |
| **D-g** | **`FrameworkParseArtifact` lives in `crates/verter_language`** (`parse_artifact.rs`): the FQ2 shape — typed `FrameworkParseCommon` (script/template/style regions, external links, diagnostics) + private erased `carrier: Arc<dyn CarrierParse>`. Downcast confined per FQ2. **Script regions are TYPED** (fix round 2): `script_regions: Vec<ScriptRegion { span, source_type: ScriptSourceType, kind: ScriptRegionKind }>` with small CLOSED `verter_language` enums `ScriptSourceType { Ts, Tsx, Js, Jsx, Dts }` and `ScriptRegionKind { Instance, Module, Frontmatter }` — the neutral answer to "is this carrier's script TS or TSX or JS?" so session-side source-type computation (`imported_eval_source_type`, a non-adapter path feeding the scheduler's cached `HostSourceData::source_type`) reads the common surface uniformly, never a carrier downcast. The `public_component_seed` field is DELETED from the struct (it was never specified — no type, producer, or consumer contract; D-n's `ComponentDefaultSynthCtx` fields subsume the need); it returns only if a vertical specifies it. | The wrapper references `FrameworkAdapterId`/`LanguageId` (D-a) and must be visible to both `verter_compiler` (producers) and `verter_session` (IndexedReady) without cycles — the leaf crate is the only home satisfying both. Per-region `source_type` is required because the live `.vue` special-case (`sfc_script_source_type` reading `ParsedSfc` at `parse.rs:122-142`) generalizes to Svelte `<script lang="ts">` and Astro frontmatter — without it the non-adapter session path would need a confinement-violating downcast. REJECTED: closed `ParsedCarrier` enum (lead follow-up: contradicts open adapter ids); raw `(FrameworkId, Arc<dyn Any>)` bag (too unstructured); unspecified seed field on the program's central neutral type (invites divergent interpretations). |
| **D-h** | **Compiler layout = lead's**: `crates/verter_compiler/src/framework_common/` (CarrierCompiler trait, vue_bridge.rs, TSX-projection helpers, shared diagnostics plumbing) + top-level `src/svelte/`, `src/react/`, `src/astro/`, `src/angular/`. `host_executor` dispatches ALL carriers through the compiler registry; Vue's entry is `vue_bridge.rs` delegating call-for-call to `parse_sfc` + the existing IDE pipeline (**flagged**: a verter_session dispatch rehousing, byte-identity-pinned; zero verter_compiler Vue-module edits). | Lead's layout verdict; co-architect's `frameworks/` nesting recorded as the rejected alternative. Single dispatch path (no dual Vue branch) honors the one-cutover build philosophy while keeping Vue behavior frozen. |
| **D-i** | **TS-side deliverable = `packages/typeinfo/src/framework-surface.ts` decode only.** The U14 `packages/component-meta/src/framework-adapter.ts` registry is NOT built here; `@verter/component-meta` + compat stay Vue-only (co-architect S2). | Cross-framework metadata's common denominator is the landed `FrameworkSurfacePayload`; per-framework `ComponentMetaAnalysis` clones would be a second metadata pipeline. REJECTED (recorded): rev2 D10 (TS registry in-program). |
| **D-j** | **Known-bug attribute form** (merging codex's required fields with the co-architect's slug + typed manifest): `#[ignore = "known-bug: <slug>; layer=<semantic\|typeinfo\|compiler-shared>; owner=<skill>; unblock=<one-line direction>"]` with a REAL discriminating body, bijective with the typed manifest (§5). | Codex's `known-bug:` prefix + owner/unblock fields are the lead's convention; the co-architect's slug + `KnownBugRow` ledger add the mechanical bijection the existing typeinfo parity ledger proves out. |
| **D-k** | **`is_synthesised_vue_default` → `is_synthesised_component_default`** (`crates/verter_session/src/resolver_core/shallow_file_state.rs:201`) — mechanical rename in B5; the flag means "synthesized component default of ANY framework". | Co-architect detail, no lead conflict; session-internal field, fully test-pinned. |
| **D-l** | **Vue-shaped leaks kept Vue-only now** (lead): `TypeInfoSurfaceMember.declared_in_macro_type_arg` (keep; doc fixed to its already-neutral meaning per rev2 D8 — no wire change), `ReductionDemand::MacroObjectSurface` (keep), `SemanticNodeData::VueMacroElements` + `HostResolvedNamedTypeKey` (keep; its `inner` cache key stays under the Vue path in B3's split — it IS Vue semantics). All three are §11 future work. New adapters must NOT add parallel resolver sidecars or macro-elements analogs. | Lead's explicit triage; no current functional need for any framework in scope. |
| **D-m** | **Vue carrier wrapper is compiler-owned; carrier privacy = public-hidden + token-gated + statically guarded** (consult verdict, fix round 1). `VueParseCarrier { parsed: Arc<ParsedSfc> }` lives in `crates/verter_compiler/src/framework_common/vue_bridge.rs` (NOT `verter_session` — unnameable from the compiler; NOT `verter_parser` — `ParsedSfc` stays parser data, the wrapper is adapter plumbing); created in B4 (type only), extended in B6 (the `CarrierCompiler` impl). `verter_language` exposes a PUBLIC `#[doc(hidden)] __carrier_downcast_ref::<T>(artifact, token: &CarrierAccessToken) -> Option<&T>` (plus an `Arc` form); `CarrierAccessToken { adapter_id, _private }` is minted ONLY inside `verter_language`, during `LanguageRegistry` carrier-row construction (D-ba — the SOLE minting authority; `FrameworkAdapterRegistry` descriptor construction RECEIVES the row's token as its registration proof and never constructs one; no public arbitrary-id constructor and no public by-id token lookup exist). Blessed wrappers: the session-side bare token-gated free helper `carrier_for::<T>(artifact, &CarrierAccessToken)` in `crates/verter_session/src/framework/ctx.rs` (lands in B4 with the cutover, D-az — `vue_parse()` routes through it; `FrameworkAdapterCtx::carrier_for::<T>()` (B5) extends the same module and routes through the same helper) and `CarrierCompilerCtx::carrier_for::<T>()` (compiler, B6). The static guard — not Rust visibility — is the enforcement authority (a literal `pub(crate)` cannot compile across the crate seam). | Resolves the dual-review carrier findings: the wrapper must be nameable from BOTH `verter_compiler` (producer) and `verter_session` (consumer), and `pub(crate)` gating in `verter_language` is unimplementable. |
| **D-n** | **`CarrierScriptFacts` is retired before it exists; script facts are semantic-owned** (consult verdict). Neutral envelope `FrameworkScriptFactSet` / `FrameworkScriptFacts { adapter_id, provider_version, stable_hash, payload: Arc<dyn FrameworkScriptFactPayload> }` lives in `crates/verter_semantic/src/analysis/framework_facts.rs` (payload access via the same token-gated hidden downcast as parse carriers). `ComponentDefaultSynth` takes a `ComponentDefaultSynthCtx { canonical_id, language, script_analysis: &ScriptAnalysisSnapshot, script_candidates: &FrameworkScriptCandidateSet, framework_parse: Option<&FrameworkParseArtifact> }` — the ctx is PARSE-DOMAIN ONLY (revised by D-au; the original stage-2-validated `script_facts` hand-off is SUPERSEDED: synth output lands in content-addressed shallow state, so resolve-env-dependent inputs are structurally banned from it — `script_candidates` is the stage-1 candidate collection); the Vue impl calls `synthesise_vue_default_value_symbol(&cx.script_analysis.macros)` unchanged (`AnalyzedMacro` stays `verter_semantic`-owned; it never crosses into `verter_compiler`, which has no `verter_semantic` dependency). B5 introduces the envelope + synth ctx + dispatch; B6 defines NO facts type. This also makes B5 ∥ B6 genuinely true (the reviewed B5→B6 type dependency is gone). | Resolves the B5/B6 dependency inversion and the unmapped-Vue-input finding; respects the verified crate graph. |
| **D-o** | **ONE host-registered TWO-STAGE `ScriptFactProvider` seam for ALL frameworks; `CarrierCompiler::analyze_script_facts` does not exist** (consult verdicts, fix rounds 1+2 — the round-1 single-stage shape required resolved import identity inside the OXC pass, which is path-resolution-agnostic per `build.rs` ("Import path resolution happens in the caller"; `AnalyzedImport.resolved_canonical_id` is populated later by `verter_session`) — implemented literally it became name-sniffing or a second resolver). **STAGE 1 — syntax-candidate capture** (provider code behind a syntax-only capture trait, invoked by a NEUTRAL dispatcher inside the ONE shallow OXC pass, `build_script_analysis_with_scope_from_program`, arena alive): trait in `verter_semantic::analysis::framework_facts` — `ScriptFactProvider { adapter_id(), provider_version(), syntax_gate() -> ScriptFactSyntaxGate, capture(cx: ScriptCandidateCx) -> Option<FrameworkScriptCandidates> }`; impls in `verter_semantic/src/analysis/framework_facts/<framework>.rs` (the crate owning the OXC pass — same precedent as Vue's `analysis::macros`); registration rows on the host registry (D-q). Stage 1 MAY inspect live OXC nodes and call `lower_ts_type`; it MAY NOT resolve imports, read capability bits (the resolved `FileLanguage` row is its only host-derived input), or emit final semantic facts. Syntax gate: a CLOSED, exact-valued descriptor `ScriptFactSyntaxGate { CarrierLanguage(LanguageId), ImportSpecifier(&'static str) }` — the file's `FileLanguage::Framework` row (carrier frameworks) OR an exact raw import SPECIFIER in the already-collected import list (package-gated frameworks, e.g. the literal specifier `"@angular/core"`); no predicate/pattern arm exists, so the `ActiveProviderIndex` indexes providers BY exact gate value and a non-matching file's active provider set is computed EMPTY before any provider invocation (D-an). Gate miss = ZERO provider AST walking, zero allocation (perf-guarded). Storage: content-addressed `framework_script_candidates` slot on the canonical per-file artifact, key `(canonical, content_hash, parse_env_hash, parser_version, file_language_id, provider_id, provider_version)` — NO capability hash, NO provider-registry fingerprint in the global `parse_env_hash` (the round-1 fold was an R21 violation: a capability flip would have invalidated every parse artifact in the workspace). **STAGE 2 — resolved-symbol validation** (session-owned, at fact-demand time, never touching an OXC arena): consumes ONLY the owned stage-1 candidates; resolves candidate import sources through the EXISTING import resolver / route facts (no second resolver, no rewalk); REJECTS userland look-alikes (a `Snippet` not resolving to the `svelte` package, an `input()` not from `@angular/core` — negative-tested per vertical); consults the DERIVED capability bits (D-r); emits validated `FrameworkScriptFacts { adapter_id, provider_version, stable_hash, payload: Arc<dyn FrameworkScriptFactPayload> }`. Stage-2 cache: content-addressed on the owner artifact identity with sub-key `(provider_id, provider_version, consumed_capability_bits, project_identity, resolve_env_hash)` (NO `type_env_hash` and NO `lib_env_hash` — stage 2 validates symbol identity/package provenance, not type meaning and not lib data; D-ah, R21 scoping, `ResolvedImportFacts` precedent; `parse_env_hash`/`parser_version`/`content_hash`/`file_language_id` ride the outer artifact key); entries publish ONLY through `SignatureAdmission::Cacheable` (`ReadSetSignature` + `validated_at_generation` on the value; overflow/cancellation/unresolved provenance → `ReturnOnly`, never published). Query-time consumers read ONLY via `FrameworkAdapterCtx::script_facts_for::<T>()` (validates before returning — internally drives stage 2 on demand). `CarrierCompiler` keeps parse/eval_source/compile_ide/template_data only — carrier and carrier-less frameworks use the SAME two-stage seam. | The OXC pass cannot know resolved import identity (verified live-code constraint); splitting capture from validation keeps the one-pass + typed-IR-only invariants AND the R21 env split: a capability flip invalidates stage-2 fact slots of exactly the affected files, a provider version bump invalidates candidates/facts, and parse artifacts are untouched by either. Respects the dep graph (`verter_compiler` cannot implement a `verter_semantic` trait). |
| **D-p** | **`VueShallowMetadataStore` migrates onto the generic `FrameworkSurfaceDtoStore` IN B5; the store is a generation-scoped, content-addressed surface memo with FULLY STRUCTURAL keys** (consult verdicts, fix rounds 1+2 — the round-1 `adapter_key_hash` u64 was collision-lossy: a lossy digest is not "keys include every deterministic input"). Key model (the live Vue hybrid, preserved; family classification per D-aq — CONTENT-ADDRESSED + fact-validated, the owner content hash deliberately IN the key; NOT a content-free query-identity cache, that vocabulary is reserved for the semantic query caches): generic columns `{ surface_kind, query_level: TypeInfoQueryLevel, canonical, owner_whole_hash }` + the adapter's STRUCTURAL key remainder — NO env-hash dims in the key (the store deliberately refuses cross-generation reuse: warm read requires `validated_at_generation == live generation`, so env is fixed within a hit; owner content rides the `owner_whole_hash` KEY column; cross-file carrier staleness rides `ReadSetSignature.facts` validation) and NO adapter/normalizer version column (process-lifetime memory — a normalizer change is a registry reset that clears the store, never versioned-key coexistence). `query_level` is a FIRST-CLASS generic column (uniform request identity — Vue proves `PublicType` vs `FullMetadata` produce different DTOs; it is neither adapter payload nor env). Storage shape: PER-ADAPTER TYPED SUB-MAPS, not one global `dyn`-keyed map — `FrameworkSurfaceStore<K, B>` per adapter (`K: Eq + Hash + Clone + Send + Sync + 'static` — the adapter's REAL structural key; Vue: `{ macro_index, macro_kind }`), erased behind the registry as `dyn ErasedFrameworkSurfaceStore` (normal `Eq`/`Hash` semantics, borrowed lookups, zero lossy digest identity; adapter key types are closed per adapter while the adapter SET stays open through registration). Value: `{ dto_bundle: Arc<dyn FrameworkSurfaceDtoBundle>, read_set_signature, validated_at_generation }`; publication ONLY through `SignatureAdmission::Cacheable` (overflow/cancellation/supersession → `ReturnOnly`); cross-adapter composition (Astro island reading a Svelte surface) MERGES the child's read-set into the parent — a non-cacheable child makes the parent non-cacheable. Byte-identity pinned by the existing Vue suites. NO dual cache substrate survives B5. | The reviewed "Vue store stays as-is" deferral was an effort rationale — disallowed. The live store's generation-gate + fact-validation model is proven; what was wrong in round 1 was ONLY the lossy key encoding and the buried query level. Guard: `framework_surface_store_key_structural`. |
| **D-q** | **Framework registry is host substrate at `crates/verter_session/src/framework/`** — `registry.rs`, `descriptor.rs`, `ctx.rs` (`FrameworkAdapterCtx`), `synth.rs` (`ComponentDefaultSynth` dispatch), `script_facts.rs` (provider registration + the D-o/D-z STAGE-2 resolved-symbol validation), `surface_store.rs` (the `FrameworkSurfaceDtoStore` substrate, D-p/D-y), `api_projector.rs` + `api_projectors/` (the D-ak `ComponentApiProjector` seam + per-framework legs), `language_classifier.rs` + `project_capabilities.rs` (D-r). `typeinfo/adapters/<framework>/` keeps ONLY the concrete `FrameworkSurfaceAdapter` impls + pure typed-IR normalizers; the executor home is `typeinfo/framework_surface/` (`mod.rs` — the executor; `vue_exec.rs` — the Vue resolution delegates B5 relocates out of `typeinfo/adapters/vue/` per D-ax, making this "keeps ONLY" clause true on the landed tree). This REFINES D-f: the lead's `typeinfo/adapters/` placement still holds for the surface-adapter impls; the registry/descriptor/ctx/synth substrate — consumed by `resolver_core` (synth selection, fact gating) and routing, not just typeinfo — moves out of the typeinfo tree. | Shallow analysis (`resolver_core`) depending on the typeinfo module tree was a module-layering smell; the descriptor registry is host-level substrate with multiple non-typeinfo consumers. |
| **D-r** | **Two-level language classification with PER-FILE-SCOPED invalidation** (consult verdicts, fix rounds 1+2). `verter_language` stays PURE: ids, static extension rows, gated-candidate descriptors only — it never reads `angular.json`, package graphs, or host config. Final classification is owned by `HostLanguageClassifier` (`verter_session/src/framework/language_classifier.rs`) composing `LanguageRegistry::classify_static(path)` with `ProjectCapabilitySnapshot` (`project_capabilities.rs`): `.html` becomes `FileLanguage::FrameworkTemplate { "angular", owner_hint }` only when the Angular capability bit is on. Invalidation rail (round-2 correction — the round-1 "fold into `parse_env_hash`" was an R21 scoping violation: a capability flip would reparse the whole workspace): (a) `ProjectCapabilitySnapshot.hash` is a hash over the DERIVED capability BITS — never raw config bytes, so a `package.json` edit that flips no bit invalidates nothing; (b) it folds into the CLASSIFICATION cache key ONLY (classification genuinely depends on it); (c) the per-file `FileArtifactStore` key gains an explicit `file_language_id` COLUMN — `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` — the resolved `FileLanguage` row is what actually changes a gated file's parse, so a capability flip invalidates exactly the files whose classification row changed (the SKILL key-composition table is updated in the landing block); (d) provider identity/versions live on the D-o candidate/fact slots, never on parse identity. NOTHING capability- or provider-shaped enters the global `parse_env_hash`. Lands in B2 (snapshot trivially empty for this whole program; the extracted Angular program adds the first capability bit, D-ac). | A leaf crate with `classify(path)` cannot express project-gated rows; splitting pure descriptors from host-gated classification keeps the leaf honest. Per-file scoping makes invalidation exact: capability flip → reclassification + affected files' artifacts only; provider bump → fact slots only; everything else untouched (R21). |
| **D-s** | **Per-kind status on the wire + operation-minimum schema gate** (consult verdict; folded into the ONE wire block B1). `FrameworkSurfaceKindEntry` gains `FrameworkSurfaceKindStatus status = 3` — `{ FrameworkSurfaceKindSupport support = 1 (UNSPECIFIED=0/SUPPORTED=1/UNSUPPORTED=2/PARTIAL=3), GraphExactness exactness = 2, repeated GraphDiagnostic diagnostics = 3 }` (reuses the EXISTING `GraphExactness` incl. `GRAPH_EXACTNESS_UNSUPPORTED = 5` + `GraphDiagnostic` messages — no parallel diagnostic vocabulary). Semantics: SUPPORTED ⇒ `members` authoritative (empty = supported-empty); UNSUPPORTED ⇒ `members` empty + `exactness = UNSUPPORTED` + ≥1 diagnostic; PARTIAL ⇒ usable subset + explaining diagnostics; UNSPECIFIED invalid in server-produced v3 payloads. A v3 framework-surface response carries EXACTLY ONE entry per known `FrameworkSurfaceKind`. Schema gating: global `SUPPORTED` stays `[2, 3]` ("server can decode this schema"), plus a per-operation minimum — `FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION = 3` enforced via `validate_schema_version_for_operation` in BOTH the envelope validation and `validate_framework_surface_request`; failures are `MalformedPayload` with detail (NOT `UnknownSchemaVersion` — v2 stays globally supported; NOT a new error oneof arm — v2 clients could not decode it); envelope/payload version mismatch on framework-surface requests is also `MalformedPayload`. Round-2 pinning: schema 2 is LEGACY-OPERATIONS-ONLY (every pre-existing op accepts `[2,3]`, asserted op-by-op); the supported-set advertisement surface is the `UnknownSchemaVersion` error payload's `server_supported_versions` field, populated from `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` via `wire_error_unknown_schema_version` (`graph.rs:288-296`) — there is NO separate server handshake-response producer in the live tree (round-3 citation correction), so the constant is the single advertisement source and it reports `[2,3]`; `validate_schema_version_for_operation` matches exhaustively over the operation discriminant with NO wildcard arm, so a future operation cannot compile without an explicit op-minimum decision. | "Empty + typed marker" was unimplementable on the live wire (`FrameworkSurfaceKindEntry` had only `kind` + `members` — verified); a v2 framework-surface request would pass generic validation yet receive an arm its decoder cannot see. |
| **D-t** | (B7 is DEFERRED per D-ab — this decision is preserved as the binding design for the deferred block.) **React (B7) is descoped and is NO LONGER the substrate proof; B5's Vue round-trip parity + TS decode is the substrate proof** (consult verdict). Ground truth (verified): ALL 9 `typeinfo_tests/jsx.rs` rows are `#[ignore]`d known-gap contracts (JSX namespace lookup, `Parameters<FC<P>>[0]`, factory inference), and `Parameters<T>[0]` tuple-index projection is itself an ignored gap (`indexed_utilities.rs`) — the plan's "in-repo proof" claim was false. B7 v1: REQUIRED step-0 probe (failing-first tests proving plainly-annotated function/arrow component first-parameter types — read from the shallow `FunctionSignature.parameters` `TypeExpr`, NOT via `Parameters<T>[0]` — and class `P` type-arg projection resolve through existing queries TODAY); PROPS ships only for the probe-proven shapes; `FC<P>`/`ComponentType<P>`/factory-inferred/`memo`/`forwardRef` are known-bug ledger rows unless the probe proves them green; SLOTS = typed unsupported + ledger row naming the JSX-namespace contract as unblocker (NO structural `ElementChildrenAttribute` lookup in B7); EMITS/MODEL/OPTIONS unchanged (typed unsupported); EXPOSE out of v1. If the probe is red, React moves behind a named follow-up core JSX/React resolution program and B7 ships registry-row + typed-unsupported surfaces only; Svelte/Astro proceed regardless. | §2.12 forbids in-program core semantic fixes; specifying B7 on registered known-gaps either shipped a ledger-row product or smuggled forbidden core work. |
| **D-u** | (Extended by D-ad — the B8a "Svelte v1 scope matrix (current-docs audit)" is the binding, complete surface enumeration; D-u's prelude design stands unchanged. Extended by D-ae — the projection TYPE ENVIRONMENT: the JSX namespace shim, the VERBATIM-lowercase event-attribute casing, and the snippet-brand declarator are pinned there.) **Svelte script typing = per-file ambient prelude; Svelte v1 is runes-first with event ATTRIBUTES primary** (consult verdict). The projected `.svelte.tsx` gets an UNMAPPED prelude inserted at output offset 0 via `CodeTransform::prepend_left` declaring the runes (`declare function $props<T = Record<string, unknown>>(): T;` + `$props.id`, `$bindable<T = never>(fallback?: T): T`, `$state`/`$state.raw/snapshot`, `$derived`/`$derived.by`, `$effect` + its FULL namespace `$effect.pre`/`$effect.tracking`/`$effect.root`, `$inspect` + `$inspect(...).with` (the returned `{ with }` object form), `$host`, `import type { Snippet } from "svelte"` — the COMPLETE Svelte 5 rune surface, so no fixture using a namespace member fails the B8c clean-type-check gate spuriously); rune CALL SITES are preserved verbatim (`let { a, b = 1, ...rest }: Props = $props()` stays — TypeScript types the destructuring/defaults/rest, Verter does not rewrite them; `$props<Props>()` works through the generic; `$bindable()` defaults to `never` so bare calls don't poison prop types). REJECTED: per-call-site CodeTransform rewrites (shift positions, degrade hover/go-to-def), project-wide shared ambient d.ts (leaks runes into non-Svelte files). Svelte 5 DOM events are ATTRIBUTES (`onclick={...}`) — the primary event path; `on:` is legacy-only coverage. The B8c validity gate is a clean TSGO/tsserver type-check + `@ts-expect-error` anti-`any` guards per fixture — OXC parse-only is NOT sufficient. | The flagship contract is TSX that TYPE-CHECKS; rune calls are compiler magic with no ambient TS declarations, and the reviewed plan was silently legacy-biased (`on:click → onClick` as primary). |
| **D-v** | (B9 is DEFERRED per D-ab — this decision is preserved as the binding design for the deferred block.) **Astro gets a real Template IR + per-file typed `Astro` prelude** (consult verdict). IR node kinds: `Fragment \| Element \| ComponentRef \| Expression \| AttrSpread \| SlotOutlet \| RawText \| RawHtml \| Comment`; `ComponentRef` stores tag span, resolved frontmatter import binding, dotted path, attrs, spreads, children, island directives (`IslandDirective { kind, value_span, raw_span }` — recorded, stripped from projected JSX props; spreads stay JSX spreads after directive filtering). Uppercase/dotted tags resolve against frontmatter value imports/bindings; lowercase = intrinsics; unresolved ComponentRef → typed diagnostic. `Astro` typing: per-file prelude `import type { AstroGlobal } from "astro"; declare const Astro: AstroGlobal<Props>` (no `Props` → `Record<string, unknown>`). Source-map rules: frontmatter 1:1; prelude/render-wrapper/slot-plumbing/`client:*` unmapped; template expressions preserve spans; tag/attr diagnostics map to tag/attr spans. Cross-adapter island invalidation needs NO new invalidation authority — component refs resolve through existing route facts + each target's synth via the one `Instantiate` identity. | "HTML-ish template + client:* stripping" was under-designed for the program's island stress test. |
| **D-w** | (B10/B11 are EXTRACTED per D-ac — this decision's full content is carried, unweakened, by `docs/arch/angular-adapter-program.md`; the summary stays here for traceability. Per D-ar(iii) the `AngularTemplateScopeDb` BUILD lands in that program's TCB block (A2), with its only consumer — the facts/surface block (A1) stays facts + surface.) **Angular selector scope is REQUIRED (option B); the TCB is scope-aware, never host-instance-only** (consult verdict — host-only/intrinsic-only is not a serious long-term architecture). B10 shallow facts (content-addressed, on the canonical artifact): `AngularComponentFact` (class slot, selector IR, standalone policy, `imports` refs, inline/external template link, inputs/outputs/models, `exportAs`, schemas), `AngularDirectiveFact`, `AngularPipeFact`, `AngularNgModuleFact` (declarations/imports/exports, schemas). NEW query-identity `AngularTemplateScopeDb` under `ProjectTypeStore`: key `{ owner_component_slot, consumed_capability_bits, project_identity, resolve_env_hash }` (content-FREE, R6; the typed capability dimension is the D-aj round-3 addition — a capability flip structurally misses entries built under other bits; `type_env_hash`/`lib_env_hash` DROPPED per D-aw/R21 — the value stores selector/scope/pipe-NAME data only, resolve-env domain; pipe `transform` SIGNATURES resolve in the TCB through the shared queries and are never stored here), value carries selector index, pipe map, schemas, diagnostics, `self_root_canonicals`, `ReadSetSignature.facts`; cold build resolves `@Component.imports` / NgModule declarations/imports/exports structurally through the shared resolver. B11 TCB contract: `null! as ChildComponent` instantiation (no constructor calls — avoids DI false errors); element matching against the scope DB (one component max, many directives); input assignability checks; `$event` synthesized from `EventEmitter<T>`/`OutputRef<T>`/DOM event maps (D-aj: `OutputRef<T>` named consistently — the common interface `OutputEmitterRef<T>` implements); `[(x)]` = input check + writeback check; `@if` → real `if` (TS narrowing), `@for` → typed item/index locals, `*ngIf/*ngFor` desugar into the SAME IR; refs (DOM → `HTMLElementTagNameMap`, component → matched instance, `exportAs` → directive facts, `ng-template` → `TemplateRef<unknown>`); pipes by scope name → `transform` signature; safe navigation → optional chaining under strict-template semantics (no deliberate `any` mode). External templates: B10 records `templateUrl → template_canonical`; SyncCoordinator sidecar ownership re-enqueues the owner TCB on edits to EITHER file. V1 exclusions are OUT-OF-SCOPE rows (host directives, custom structural-directive guards, full schema/custom-element typing, DI provider context, `@defer`, animations, forms/CVA inference); known-bug LEDGER rows are reserved for pre-existing shared-resolver failures Angular surfaces — the two lists never mix. | Child-component bindings through the compilation scope are the dominant Angular type-check value; the reviewed plan hand-waved past selectors/scopes entirely, and "TSX sidecar, one function per component" was not yet a design. |
| **D-x** | **Virtual-file naming is a ROLE-SEPARATED registry column, not per-consumer convention; the column is TOTAL over the live virtual-file roles** (revised, fix round 2 — the single-`primary_suffix` shape could not encode the live Vue dual-virtual-file model; revised again, fix round 4 (D-al) — the round-2 column still could not encode the live TESTING role, the same defect class one role further out; the role set is now enumerated from a live-tree sweep and the column is total over it). `FrameworkAdapterDescriptor` gains `virtual_file_naming: Option<VirtualFileNaming { ide: Option<IdeSuffixPolicy>, api_suffix: Option<&'static str>, testing_api_suffix: Option<&'static str>, sidecar_suffixes: &'static [&'static str] }>` with a small CLOSED `IdeSuffixPolicy { Fixed(&'static str), JsxConditional { jsx: &'static str, non_jsx: &'static str } }` (`ide: None` = the owner file is served directly with no IDE projection — Angular's component owner is a real `.ts`). **The live-tree role enumeration the column is total over (D-al)**: (1) **ide** — `{canonical}.tsx`/`.jsx`, content `get_ide`; (2) **api** — `{canonical}.ts`, content `get_public_api` (`PublicApiMode::Public`); (3) **testing-api** — `{canonical}.__verter_test.ts`, content `get_public_api_with_mode(Testing)` (`virtual_file_pipeline.rs:1484`), named by ts-plugin `VUE_TEST_TS_REGEXP` (`utils.ts:4`) / `toVueVirtualFileName(f, "testing")` (`utils.ts:44-46`) / NAPI `getPublicApi(f, "testing")` — a MODE of the api producer, so the structural rule `testing_api_suffix.is_some() ⇒ api_suffix.is_some()` is pinned; (4) the **`.d.ts` accepted-spelling alias** — `{canonical}.d.ts` (`VUE_D_TS_REGEXP`, `utils.ts:3`) is a ts-plugin LOOKUP-ACCEPTANCE spelling normalizing to the api surface (mode `public`), derived UNIFORMLY as `{carrier_ext}.d.ts` from the carrier-extension set — an acceptance rule, not a column field; (5) **sidecars** — additional per-component virtual files (`.ngtcb.tsx`, extracted Angular program). NOT a role: `ProviderPathKind::Shadow` (`provider_sync.rs:5-9`) is the IDENTITY path for plain `Script` files (`provider_id_for_source` returns the file itself) — no suffix, no carrier, outside the column by construction (recorded so the enumeration is checkably total over `ProviderPathKind`). ALL suffixes have explicit APPEND-TO-FULL-CANONICAL semantics (`App.vue` + `".ts"` → `App.vue.ts`; `Foo.svelte` + `".tsx"` → `Foo.svelte.tsx` — no stem rewriting). Ground truth pinned (verified; round-3 citation correction — the previously cited `sync_coordinator.rs:787/:868` lines are `#[cfg(test)]` fixture constructions, not production): the live LSP maintains TWO virtual files per `.vue`; the PRODUCTION derivations are `crates/verter_workspace/src/resolver.rs:241` (`provider_id_for_source` — appends `.ts`, the api path) and `:256` (`provider_ide_id_for_source` — appends `.tsx`/`.jsx` by JSX-ness, the ide path), consumed at `provider_sync.rs:161-162` (`ProviderSyncState { ide_path, api_path }`) and `server_utils.rs:165-171`, PLUS the production local re-derivation `provider_sync.rs:350` (`open_unresolved_vue_state` formats the JSX-conditional ide path inline — exactly the per-consumer re-derivation this column retires); the typescript-plugin consumes the API naming. Rows: Vue = `{ ide: JsxConditional { ".jsx", ".tsx" }, api_suffix: Some(".ts"), testing_api_suffix: Some(".__verter_test.ts"), sidecar_suffixes: [] }` (encodes the live derivations exactly — characterization test against the `resolver.rs:241/:256` + `provider_sync.rs:161-162/:350` production sites AND the ts-plugin testing-role sites `utils.ts:4/:44-46`); Svelte = `{ ide: Fixed(".tsx"), api_suffix: Some(".ts"), testing_api_suffix: None, sidecar_suffixes: [] }` — **Svelte SHIPS the api virtual file in v1** (`Foo.svelte.ts`; a TS file importing `./C.svelte` under tsserver needs exactly the api-path mechanism, the B8c ts-plugin story; its CONTENT producer is the D-ak `ComponentApiProjector` leg), and the TESTING surface is explicitly OUT-OF-SCOPE v1 for Svelte (D-ak: `PublicApiMode::Testing` is the Vue-Test-Utils `<script setup>` bindings surface, not a framework-neutral concept — `get_public_api_with_mode(Testing)` returns `None` for Svelte and the generated ts-plugin module never forms `.svelte.__verter_test.ts`); Angular = `{ ide: None, api_suffix: None, testing_api_suffix: None, sidecar_suffixes: [".ngtcb.tsx"] }` (the sidecar appends to the owner component's canonical — `<owner>.ngtcb.tsx`; the Angular row itself lands in the extracted Angular program, D-ac — the COLUMN design here is what that program consumes). The ts-plugin regexps (B8c), LSP `sync_coordinator` mapping, and per-framework `ide/` modules all derive naming from this one column. The TS mirror is GENERATED AND BYTE-PINNED (round-3 — every other generated TS surface in this plan is byte-pinned; this one is too): the Rust registry rows are the single authority; a freshness test (`virtual_file_naming_ts_freshness`, the `typeinfo_proto_ts_freshness.rs` pattern) renders the TS constant module `packages/typescript-plugin/src/generated/virtual-file-naming.ts` from the descriptor table and byte-compares it against the checked-in file (regeneration via the same test under an explicit update flag); a hand-edit or a registry-row change without regen fails the gate. | Review found three naming conventions about to live in three places; round-2 review found the one-suffix column could not encode the live dual-file (`ide`+`api`) + JSX-conditional reality and was internally inconsistent about append semantics; round-4 review found the SAME class again at the testing role (`VUE_TEST_TS_REGEXP` underivable from the column) — closed this time by sweeping the live tree for EVERY role and making the column total over the enumeration, with the `.d.ts` acceptance spelling and `ProviderPathKind::Shadow` explicitly classified so nothing is silently outside it (D-al). Role-separated columns + uniform append semantics encode the live model losslessly. |
| **D-y** | **Round-2 cache-key reconciliation (fix-round-2 codex consult).** The `FrameworkSurfaceDtoStore` is pinned as a generation-scoped, CONTENT-ADDRESSED + fact-validated surface memo with fully structural keys (family classification per D-aq: owner content/version identity deliberately rides the `owner_whole_hash` KEY column — the content-addressed family, NOT content-free query identity): per-adapter typed sub-maps behind `dyn ErasedFrameworkSurfaceStore`; generic columns `{ surface_kind, query_level, canonical, owner_whole_hash }` + the adapter's typed `Eq + Hash` key remainder; NO lossy digest key component, NO env-hash dims (strict same-generation gate + `ReadSetSignature.facts` make env identity value-side), NO adapter/normalizer version column (process-lifetime memory; a normalizer change clears the store); publication only via `SignatureAdmission::Cacheable`; cross-adapter reads compose read-sets (non-cacheable child ⇒ non-cacheable parent). Authoritative spec: D-p (revised). Guard: `framework_surface_store_key_structural`. | Resolves the paired round-2 P0/P1 (lossy `adapter_key_hash`; env dims hidden in an opaque hash; buried query level) with ONE key model that preserves the live `VueMacroDtoKey` store's proven semantics. |
| **D-z** | **Round-2 seam reconciliation (fix-round-2 codex consult).** The `ScriptFactProvider` seam is TWO-STAGE: stage-1 syntax-candidate capture inside the one OXC pass (live AST + `lower_ts_type`; no import resolution, no capability reads; content-addressed `framework_script_candidates` slot keyed `(canonical, content_hash, parse_env_hash, parser_version, file_language_id, provider_id, provider_version)`), stage-2 session-owned resolved-symbol validation at fact-demand time (existing resolver/route facts; userland look-alike rejection; derived capability bits; sub-key `(provider_id, provider_version, consumed_capability_bits, project_identity, resolve_env_hash)` — `lib_env_hash` dropped per D-ah; `SignatureAdmission`-gated). Env scoping: capability hash (over DERIVED bits) keys classification only; `FileArtifactStore` gains the per-file `file_language_id` column; nothing capability/provider-shaped enters global `parse_env_hash`. Authoritative spec: D-o + D-r (revised). Guards: `script_fact_capture_is_syntax_only`, `script_fact_providers_zero_cost_on_miss` (strengthened with the exact-gate-indexed `ActiveProviderIndex`, D-an). | Resolves the paired round-2 P0/P1 (providers required resolved import identity inside the path-resolution-agnostic OXC pass; R21 violation folding capability/provider identity into global `parse_env_hash`) with ONE staged design matching the live `AnalyzedImport.resolved_canonical_id` lifecycle. |
| **D-aa** | **No new `FrameworkTag` values land in this program; a tag lands ONLY with its vertical; B1 pins the tag-semantics doc comment on the wire.** The D-b item (ii) additions (`FRAMEWORK_TAG_ASTRO = 6`, `FRAMEWORK_TAG_ANGULAR = 7`) are DROPPED from B1. Svelte needs no addition — `FRAMEWORK_TAG_SVELTE = 2` already exists on the live wire (verified). B1 instead adds proto doc comments on `FrameworkTag` (and mirrors the sentence in `FrameworkAdapterDescriptor` rustdoc + the `/framework-adapters` skill): **"a `FrameworkTag` value's existence is NOT a support guarantee; support is asserted only by a registered adapter (and surfaced per-request via `FrameworkSurfaceKindStatus`)"**. Future tag additions ride each vertical's own wire decision under the closed-contract rules (proto3 enum value additions are decode-safe for unknown values; whether a given addition warrants a schema bump is decided by that vertical's program). The doc-comment change flows into the regenerated byte-pinned TS bindings B1 already produces. | Owner rule applied with the recommended resolution. The live wire is itself the proof of the hazard AND of the staging model: `FRAMEWORK_TAG_REACT = 3` and `FRAMEWORK_TAG_SOLID = 4` exist today with NO adapter behind them — tags already do not imply support, so the semantic must be written down, and adding two more unsupported tags ahead of their verticals would compound the ambiguity for zero functional gain (no in-scope code path reads them). `framework_registry_complete` (B5) is the runtime enforcement: every wire tag maps to a registered adapter OR an explicit registered out-of-scope/deferred row. |
| **D-ab** | **Program re-scope: "framework adapter substrate + Svelte proof".** Execution scope = B1–B6 + B8a/B8b/B8c + B12 (reduced to docs/skills updates, the guard sweep over landed guards, the `.vue`-literal sweep, and STOP-gate/parity-manifest reconciliation for Svelte only). React (B7) and Astro (B9) are DEFERRED: their full designs are preserved verbatim in the "Deferred Verticals" section, each headed by the evidence gate that reopens it — **reassess after the Svelte proof with evidence from the landed seams** (registry/executor behavior, `ScriptFactProvider` two-stage cost + correctness, DTO-store multi-adapter behavior, virtual-file naming/LSP wiring, known-bug ledger yield). Deferred ≠ deleted: no design content was removed, no round-1/round-2 review resolution (D-y/D-z cache keys, D-s schema gate, D-x naming, D-g `ScriptRegion` typing, zero-overhead bitset/`ActiveProviderIndex`) is weakened — the substrate still lands fully framework-open. Per-vertical guards owned by deferred blocks ride with them; re-run-per-vertical guards run next with the next landed vertical (B8a). | Owner directive. The substrate seams are designed framework-open and are proven by Vue parity (B5) + one real non-Vue carrier vertical (Svelte); a second and third simultaneous vertical adds review surface without adding substrate evidence. Evidence-gated reopening replaces speculative parallel execution. |
| **D-ac** | **Angular is EXTRACTED into `docs/arch/angular-adapter-program.md`** — a standalone follow-up program document carrying the FULL former-B10/B11 designs (selector-scope facts, `AngularTemplateScopeDb`, Template IR + TCB contract, the two-stage `ScriptFactProvider` Angular rows incl. stage-2 `@angular/core` resolution validation, `input()`/`output()`/`model()` + decorator surface mapping, gated `.html` classification, TCB sidecar virtual file), its explicit dependencies on the substrate seams THIS program lands, and an explicit go/no-go section (criteria: Svelte vertical landed; Astro reassessment outcome recorded; the named seam evidence reviewed). The main plan keeps a one-paragraph pointer where B10/B11 stood. `FRAMEWORK_TAG_ANGULAR` and the `.html` gated-candidate `LanguageRegistry` row land in THAT program, not this one (D-aa; B2 ships the gated-row MECHANISM with an empty `ProjectCapabilitySnapshot`). | Owner directive. `input()`/`output()`/`model()` are real current Angular surface and the design must not rot — but scope-aware TCB generation (compilation-scope DB + sidecar virtual file + template IR + strict-template semantics) is a program-sized effort that deserves its own go/no-go after Svelte (and the Astro reassessment) prove the registry model, rather than trailing as two blocks of an already-large program. |
| **D-ad** | **Svelte current-docs audit (Svelte 5.56.3, docs read 2026-06-10) — the B8a "Svelte v1 scope matrix (current-docs audit)" is the binding, COMPLETE surface enumeration for B8a/B8b/B8c.** It extends (never duplicates) D-u: every D-u row is incorporated; surfaces that did not exist or were not enumerated when D-u was written are now EXPLICITLY SUPPORTED or EXPLICITLY OUT-OF-SCOPE v1. Headline outcomes: `{@attach …}` (5.29, stable) = SUPPORTED (prelude-checker projection typed via `svelte/attachments`' `Attachment<E>`); await-expressions (5.36, EXPERIMENTAL behind `experimental.async`, docs warn of breaking changes outside semver-major) = OUT-OF-SCOPE v1 with parse-without-crash + typed-unsupported diagnostic + void-checked expressions, revisit at stabilisation; declaration tags `{const …}`/`{let …}` (5.56 — `{@const}` is now documented legacy) = SUPPORTED; `$state.eager` (5.41), `$inspect.trace` (5.14), `$effect.pending` = SUPPORTED prelude declarations; `class` object/array clsx forms (5.16) = SUPPORTED via a `ClassValue`-typed prelude checker; `<svelte:boundary>` (5.3) = SUPPORTED; function bindings `bind:x={get, set}` (5.9), `style:` directive, the wide `bind:` family beyond `value`/`checked`/component-`bind:prop`, `<svelte:fragment>` = OUT-OF-SCOPE v1 with exact behaviors recorded. B8c's prelude list and B8a's parser scope are updated to match. The matrix records the audited Svelte version; a future version refresh re-runs the audit before any scope claim changes. Round-4 completion (D-ap): the docs-ToC Styling section (scoped/global styles, custom properties, nested `<style>` elements) and the Runtime `hydratable()` page are explicitly dispositioned — the matrix is total over the current docs ToC, re-verified against the live docs in round 4. | Owner directive: every current-docs surface must be explicitly supported or explicitly out-of-scope before B8 executes. The audit was taken against the live official docs (svelte.dev/docs/svelte/*), not training data; `{@attach}` and await-expressions were owner-named and appear explicitly. |
| **D-ae** | **The Svelte projection TYPE ENVIRONMENT (fix-round-3 codex consult; completes D-u/D-ad — the B8c clean-type-check gate is now implementable): per-file `@jsxImportSource` pragma to a Verter-owned Svelte JSX shim; event attributes project VERBATIM lowercase; snippets bridge the `Snippet` brand through a prelude declarator.** (a) The `.svelte.tsx` prelude OPENS with `/** @jsxImportSource @verter/svelte-jsx */` — the pragma overrides the provider's project-level `jsxImportSource: "vue"` (`crates/verter_lsp/src/extension_provider.rs:932`) for THIS file only, even under `jsx: "preserve"` (consult-verified against TS 5.4/5.8/6.0; a TSGO pragma-parity fixture is a B8c gate precondition). The shim is a VERTER-OWNED d.ts asset (`@verter/svelte-jsx/jsx-runtime.d.ts` + `jsx-dev-runtime` re-export; repo home, distribution, and per-consumer locating pinned by D-av) path-mapped into the inferred project through the provider's existing `paths` configuration — its `JSX` namespace: `Element = ReturnType<Snippet>`, `ElementClass {}`, `ElementAttributesProperty { $props: {} }` (component-tag props check against the class-shaped synth's `$props` — a `.vue` component imported into a `.svelte` file checks through the same contract, no cross-framework branch), `IntrinsicElements extends SvelteHTMLElements` (from `svelte/elements` — Svelte-true element/attribute typing). (b) The `onclick → onClick` rename is DROPPED: Svelte 5 event attributes project VERBATIM lowercase (`SvelteHTMLElements` is lowercase — the rename only type-checked under a Vue/React-style namespace and contradicted D-u's verbatim philosophy); legacy `on:click` NEVER becomes `onClick` either — unmodified legacy directives carry the namespaced attribute verbatim (typed by `SvelteHTMLElements`' quoted keys), modifier forms lower through a typed helper against the base quoted key. (c) `{#snippet mySnip(x: T)}` projects as `const mySnip = __verter_snippet((x: T) => (…JSX…))` with prelude declarator `__verter_snippet<Params extends unknown[]>(render: (...args: Params) => unknown): Snippet<Params>` — the body type-checks normally, the binding carries the BRANDED `Snippet<[T]>` so `{@render mySnip(v)}` and snippet-as-prop assignability both check; a plain function passed where `Snippet<[T]>` is expected stays an error (discriminating). (d) Missing-`svelte`-package behavior fails CLOSED: module-not-found diagnostics surface (no ambient stubs for the `svelte` package, no `any` fallback — ambient stubbing would invalidate the anti-`any` gate) plus a typed Verter diagnostic `svelte-package-missing` on the source file. REJECTED: file-local `declare namespace JSX` (a module-scoped namespace is not consulted by JSX lookup; `declare global` would pollute the whole inferred project incl. Vue files); pointing the pragma at the raw `svelte` package (no guaranteed jsx-runtime export). NAMED FALLBACK (only if the TSGO pragma-parity fixture fails): function-call projection of elements/components (no JSX namespace consulted) — a STOP-and-redesign decision, never a silent degrade. | Round-3 review (fable P1-1): the flagship gate was unimplementable as written — the live provider hard-codes the Vue JSX environment, `SvelteHTMLElements` is lowercase while the plan renamed `onclick → onClick`, nothing provided `ElementAttributesProperty { $props }`, and the `Snippet` brand made snippet-as-prop fixtures unable to pass. One pragma + one Verter-owned shim resolves all four halves coherently while leaving Vue's environment untouched. |
| **D-af** | **B4 covers the FULL production `ParsedSfc` carrier set — seven struct carriers + the threading APIs (live-tree sweep, fix round 3).** The production parse-payload class is confined to `verter_session` (verified: ZERO `ParsedSfc`/`cached_parse` references in `verter_scheduler`/`verter_workspace`/`verter_ffi`/`verter_napi`/`verter_wasm`/`verter_lsp`/`verter_mcp` production code). B4 dispositions EVERY site: the seven `cached_parse` struct fields — `IndexedReady` (project_type_store.rs:158), `RouteOwnedShallowEntry` (:532), `HostSourceData` (host_executor.rs:30), `CompileInput` (types.rs:1852), `EffectiveFileState` (types.rs:2110), `ContentOverrideWithParse` (types.rs:2124), `ExternalTypeResolutionInputs` (host_manage.rs:440) — are REPLACED by `framework_parse: Option<Arc<FrameworkParseArtifact>>`; every neutral threading API passing `Option<&ParsedSfc>`/`Option<Arc<ParsedSfc>>` goes neutral; Vue-semantic leaves keep `&ParsedSfc` CONFINED to the Vue bridge (reached only via the blessed `vue_parse()` downcast); the `route_owned_snapshot_cached_parse_hits` provenance-counter family is RENAMED `route_owned_snapshot_parse_artifact_hits` (semantics unchanged); the direct `parse_sfc` PRODUCER call sites — the route-owned cold parse, the template-analysis cold/merged-source re-parses, the on-demand `*_from_source` builders — are dispositioned per D-bb. The full file:line disposition table lives in B4. The retired-symbol gate bans the `cached_parse` token workspace-wide in production; NEW static guard `parsed_sfc_confined_to_vue_bridge` allowlists BOTH the `ParsedSfc` type token AND the `parse_sfc` producer-call token (D-bb) to `verter_parser`/`verter_compiler` plus the named session Vue-bridge files; the sweep dispositions the direct `parse_sfc` PRODUCER call sites too — the route-owned cold parse (`host_resolve/route_owned_shallow.rs:303-306`), the template-analysis cold/merged-source re-parses (`host_manage/analysis_io.rs:93/:100/:237/:244`), and the on-demand `*_from_source` builders (`parse.rs:755/:773`). Characterization pins `HostSourceData.source_type`, content overrides (`ContentOverrideWithParse`/`EffectiveFileState`), eval-source building, and route-owned shallow state byte-identically. | Round-3 review (codex P0): the reviewed B4 deleted only the two `project_type_store.rs` fields — it could not pass its own retired-symbol gate while five more production carriers (and the threading APIs) kept the Vue-only parse payload alive in the session authority. The sweep dispositions the CLASS, not just the cited instances. |
| **D-ag** | **Adapter registration is TWO structural legs — carrier legs and the surface leg; an adapter id may be registered with a typed `SurfaceRegistration::Deferred` arm before its surface adapter lands.** `FrameworkAdapterRegistry` rows are `FrameworkRegistration { descriptor, surface: SurfaceRegistration }` with CLOSED `SurfaceRegistration { Adapter(Arc<dyn FrameworkSurfaceAdapter>), Deferred }`; carrier/synth/script-fact-provider legs ride the descriptor. The executor serves a `Deferred` surface leg STRUCTURALLY: one `FrameworkSurfaceKindEntry` per known kind, `UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` + a diagnostic naming the adapter's surfaces as not-yet-registered (the D-s vocabulary — no hand-rolled error path, never a panic). B8a registers Svelte's carrier legs with `surface: Deferred`; B8b REPLACES the arm with the real `SvelteFrameworkAdapter` (one field flip, no dual path). `framework_registry_complete` distinguishes the legs: every registered adapter id has a descriptor + EITHER a surface adapter OR the explicit structural `Deferred` arm; every wire tag maps to a registered adapter or an explicit out-of-scope/deferred TAG row; carrier-leg compiler completeness stays B6's `carrier_descriptors_have_compilers`. | Round-3 review (codex P1): B8a registered the Svelte row while "surfaces unsupported with typed errors" was prose-only — the intermediate state was conventional, not structural, and contradicted the guard's "descriptor plus surface adapter" clause. REJECTED: a throwaway `UnsupportedSurfaceAdapter` impl that B8b would delete — the structural enum arm expresses the same state without a disposable type. |
| **D-ah** | **The stage-2 script-fact cache sub-key DROPS `lib_env_hash`** — final sub-key `(provider_id, provider_version, consumed_capability_bits, project_identity, resolve_env_hash)`. Stage 2 validates symbol identity / package provenance through the import resolver — resolve-env domain; no stage-2 value reads lib data. R21's scoping rule (`lib_env_hash` enters a key ONLY when the value depends on lib data) and the `ResolvedImportFacts` precedent both exclude it; over-keying is spurious whole-workspace fact invalidation on every lib change. If a future provider's stage-2 validation genuinely consults lib data, that provider's program records the dependence and adds the dim THEN. Applies to D-o/D-z/B5 and the extracted Angular program's A1 verbatim. | Round-3 review (fable P2-2): the sub-key carried `lib_env_hash` with no recorded lib dependence — exactly the unscoped-dim class R21 forbids; the plan's own `type_env_hash` exclusion argument applies identically. |
| **D-ai** | **`FrameworkSurfacePlan` is a CLOSED typed demand vocabulary mapping 1:1 onto the framework-surface EXECUTOR's private resolve operations (placement revised by D-as — the ops live on the executor-private resolve surface, never on the adapter ctx).** `FrameworkSurfacePlan { items: Vec<PlannedResolve> }`; `PlannedResolve { kind: FrameworkSurfaceKind, demand: PlannedDemand }`; CLOSED enum `PlannedDemand { PublicTypeInstance { canonical } → instantiate_public_type, MacroPayload { owner, selector } → resolve_macro_payload, PathProjection { base, path, mode } → project_path, ShallowSurface { node } → shallow_surface }`. NO variant may carry source text, raw byte ranges standing in for source, OXC handles, closures, or raw `SemanticQueryKey`s; there is NO `Custom`/`Raw`/escape arm. The executor's dispatch matches EXHAUSTIVELY over `PlannedDemand` (no wildcard arm) so a new demand variant cannot compile without an explicit executor decision — and per D-as a new demand variant is the ONLY way an adapter gains access to more semantic data; a whole-enum unit test pins the 1:1 executor-operation mapping (B5 test); the adapter-visible ctx and the executor-private resolve surface are split per D-am/D-as. | Round-3 review (fable P2-3): the keystone trait's central currency was named but never shaped — an open-ended plan vocabulary would degrade "one engine by construction" back to "enforced by guard only". |
| **D-aj** | **Angular-program round-3 reconciliations (recorded here for traceability; carried authoritatively by `docs/arch/angular-adapter-program.md`).** (i) `AngularTemplateScopeKey` gains the typed `consumed_capability_bits` dimension — `{ owner_component_slot, consumed_capability_bits, project_identity, resolve_env_hash, type_env_hash, lib_env_hash }` (later narrowed by D-aw: `type_env_hash`/`lib_env_hash` dropped per R21 — the value is resolve-env domain), still content-free (derived capability bits are env-shaped derived config, never content/version hashes) — a capability flip structurally misses every scope entry built under other bits; flip-on/flip-off tests cover the scope DB, surfaces, and TCB inputs (closing the warm-entry-survives-capability-off hole). (ii) A0 pins the schema rule explicitly: `FrameworkTag` VALUE additions do NOT bump `TYPEINFO_GRAPH_SCHEMA_VERSION` — `FrameworkTag` is a proto3 OPEN enum, not one of the four closed `oneof` taxonomies whose variant additions the typeinfo wire contract's bump rule governs; the compatibility proof + decode-compat tests land with A0. (iii) The `"angular_template"` FFI accepted string moves A1 → A2 (lands together with the `.html` gated registry row — B2's accepted-string/registry-row pairing rule; no dangling kind between A1 and A2). (iv) The output payload carrier is named `OutputRef<T>` consistently (the common interface `OutputEmitterRef<T>` implements) in the Angular doc §5/A1/A2 and the D-w mirror here. | Round-3 review (codex P1 ×2 — capability invalidation hole, A0 schema punt; fable P2-4/P3-5). Each fix follows the substrate's own rails: the stage-2 sub-key's `consumed_capability_bits` precedent, D-aa's per-vertical wire-decision rule made explicit instead of re-deferred, and B2's pairing rule for FFI strings. |
| **D-ak** | **The api virtual file's CONTENT producer is a session-owned `ComponentApiProjector` registration leg; Vue's leg is the existing extraction, byte-pinned; Svelte's leg is a pure shallow-declaration renderer (fix-round-4 codex consult).** Ground truth: the SOLE live producer `VerterHost::get_public_api` / `get_public_api_with_mode` (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1472/:1484`) is Vue-hard-gated (`file_kind != FileKind::VueSfc → None`) macro-only extraction feeding `sync_coordinator.rs:246`, `workspace_scanner.rs:790`, four `background_drain.rs` sites, `sync_orchestration.rs` ×4, `documents/mod.rs:485`, `component_resolve.rs` ×3, `verter_tsc/checker.rs`, `verter_type_runtime` tsserver ipc ×4, NAPI (`getPublicApi`, modes `public`/`testing`), WASM — the host method is the single entry and STAYS the single entry (every consumer untouched). B5 lands the seam: trait `ComponentApiProjector { render_api(cx: ComponentApiProjectorCtx<'_>) -> Option<TscResponse> }` in `framework/api_projector.rs`; an api-projector leg rides the registration row (like the carrier/synth legs, D-ag); `get_public_api_with_mode` dispatches through the registry after classification + registry lookup (the Vue hard gate is REPLACED in the same change — no dual branch); the Vue leg (`framework/api_projectors/vue.rs`) delegates to the mechanically-extracted legacy producer (`render_vue_public_api_legacy`) — the ENTIRE existing flow (`cached_tsc_extract` on DerivedRawState, `extract_tsc_state`, `generate_tsc_from_state`, `collect_external_types_from_loaded_files`, `sync_transitive_macro_type_dependencies`) unchanged, byte-pinned for BOTH `PublicApiMode`s incl. source maps + external macro types. B8a lands the Svelte leg (`framework/api_projectors/svelte.rs`): a declaration shim rendered from the carrier's SHALLOW inventory + the D-n synthesized default symbol/export inventory — the D-at type-only import/re-export PRELUDE (rendered from the shallow script/module import facts for every preserved reference — named aliases, default imports, namespace imports, re-exports; unused imports dropped), `type __VerterProps` (the `$props()` type / legacy export-let object; refs PRESERVED, never eagerly inlined — shallow-by-default applies to the shim; the prelude is what makes the preserved refs resolve in the `.svelte.ts` module context), `interface __VerterInstance { $props: __VerterProps; …instance-script exports }`, `declare const __VerterComponent: { new (...args: any[]): __VerterInstance }`, `export default __VerterComponent`, `<script module>` exports as top-level named declarations; NO `Instantiate`/semantic dispatch/OXC at render time — the api file is declaration TEXT tsserver resolves itself (static-guarded: non-Vue api projectors never call semantic dispatch/OXC/query-time resolution). Testing mode: Svelte returns `None` (D-x/D-al `testing_api_suffix: None`). Cache: NO new final-content cache for the Svelte leg (pure cheap render over already-cached shallow inputs; `cached_tsc_extract` is Vue extraction state, never reused); a future cache, only if profiling demands it, is a dedicated content-addressed final-shim slot (canonical + owner content identity + adapter id + projector version + mode + profile/effective-source identity + consumed script-fact stable hash). `framework_registry_complete` gains the api-leg clause: every descriptor with `virtual_file_naming.api_suffix = Some(_)` has a registered api-projector leg when its carrier legs register (Vue at B5, Svelte at B8a). REJECTED: `CarrierCompiler::render_api` (the Vue producer needs session-only resolver context, external macro-type collection, host caches — a dependency inversion for `verter_compiler`); one mandatory generic renderer (Vue must opt out for byte identity; future frameworks may need declaration quirks). | Round-4 review (fable P1-1): B8c's headline e2e (a TS file importing `./C.svelte` resolving through `C.svelte.ts`) could not go green — `get_public_api` returns `None` for non-Vue, the api sync silently skips, and no Changes section named a producer. The plan contained zero occurrences of `get_public_api`/`PublicApiMode`. |
| **D-al** | **The `VirtualFileNaming` column is TOTAL over the live virtual-file role enumeration; the testing role is a first-class column field.** Live-tree sweep (round 4): **ide** (`.tsx`/`.jsx`, `get_ide`), **api** (`.ts`, `get_public_api` Public), **testing-api** (`.__verter_test.ts` — `PublicApiMode::Testing` at `virtual_file_pipeline.rs:1484`, `VUE_TEST_TS_REGEXP` at `utils.ts:4`, `toVueVirtualFileName(f,"testing")` at `utils.ts:44-46`, NAPI `getPublicApi(f,"testing")`), the **`.d.ts` accepted-spelling alias** (`VUE_D_TS_REGEXP`, `utils.ts:3` — ts-plugin lookup acceptance normalizing to the api surface, derived uniformly as `{carrier_ext}.d.ts`; an acceptance rule, not a column field), **sidecars** (`.ngtcb.tsx`, extracted Angular program); `ProviderPathKind::Shadow` (`provider_sync.rs:5-9`) classified as the plain-`Script` IDENTITY path — no suffix, not a carrier virtual-file role, outside the column by construction. Column gains `testing_api_suffix: Option<&'static str>` with the structural rule `testing_api_suffix.is_some() ⇒ api_suffix.is_some()` (testing is a MODE of the api producer, D-ak); Vue `Some(".__verter_test.ts")`, Svelte `None` (testing surface explicitly out-of-scope v1, D-ak), Angular `None`. The D-x characterization test and the generated byte-pinned `virtual-file-naming.ts` module render ALL roles incl. the testing regexp and the `.d.ts` acceptance spellings — all four `utils.ts:1-4` constants are now derivable. Authoritative spec: D-x (revised). | Round-4 review (fable P1-2): the round-2 column could not encode the live testing role, so one of the four constants B8c explicitly commits to deriving (plus two named consumers) had no derivation — the same defect class as round 2's one-suffix fix, recurring one role further out; closed by making the enumeration total over the live tree, not by adding one more field blind. |
| **D-am** | **`FrameworkAdapterCtx` is a CLOSED, enumerated op surface; `ensure_indexed_ready` is executor/session-private; adapters never reach raw/eval source or content snapshots.** (Revised by D-as: the four resolve ops `instantiate_public_type` / `resolve_macro_payload` / `project_path` / `shallow_surface` and `export_graph` — originally enumerated HERE as ctx ops — are EXECUTOR-PRIVATE; D-as is the authoritative op placement.) The ctx's public methods are EXACTLY: `carrier_for::<T>(canonical)` (the ctx drives artifact materialization INTERNALLY and hands back only the adapter's own token-gated typed carrier — never the artifact wrapper, never `IndexedReady`) and `script_facts_for::<T>(canonical)`. Banned from adapter reach (the raw-source class, enumerated): `ensure_indexed_ready`, `IndexedReady` (the live struct carries `raw_source` + `eval_source`, `project_type_store.rs:147-160`), `HostSourceData`, `EffectiveFileState`, `StoreView`, content overrides — plus, per D-as, every semantic resolve entry point. Guard `framework_adapter_ctx_closed_surface`: static-grep over `typeinfo/adapters/**` + an API-surface pin test enumerating the ctx's public methods. | Round-4 review (codex P1): B5's ctx op list handed adapters `ensure_indexed_ready` while invariant 1 promises adapters never get raw source — the live `IndexedReady` exposes both source snapshots, so the promise was violated by the plan's own op list. Closed as a CLASS: the op surface is enumerated and pinned, not just the one offending op removed. (Round 5 closed the remaining class member — resolve ops on the ctx — via D-as.) |
| **D-an** | **`ScriptFactSyntaxGate` is a CLOSED exact-valued vocabulary and the `ActiveProviderIndex` indexes providers BY gate value — `by_carrier_language` + `by_import_specifier` exact-match maps; no per-`FileLanguage::Script` provider list exists.** `ScriptFactSyntaxGate { CarrierLanguage(LanguageId), ImportSpecifier(&'static str) }` — no predicate/pattern arm, so the index is total over gates by construction. Per-file active set: carrier files → `by_carrier_language[row]`; `Script` files → union of `by_import_specifier[spec]` over the file's already-collected raw import specifiers, behind an `is_empty()` fast path (with no specifier-gated provider registered — this whole program's state, Svelte's gate being `CarrierLanguage` — even the per-specifier lookups are skipped). A non-matching script file's provider set is computed EMPTY before any provider invocation, gate evaluation, or fact-container allocation; a specifier-gated provider (the extracted Angular program's `"@angular/core"` row is the stress case) is reachable only through its exact specifier key. Applies verbatim to the Angular program's A1 provider. Guard: `script_fact_providers_zero_cost_on_miss` (strengthened, §6). | Round-4 review (codex P1): the round-2 per-`FileLanguage`-row pre-filtered list would have put a specifier-gated provider on EVERY `Script` file's list — per-file per-provider gate evaluation is not zero overhead; the zero-overhead claim was structurally unproven exactly where it matters most (the deferred Angular provider over a large TS workspace). |
| **D-ao** | **The `.svelte` `LanguageRegistry` row lands in B2, PAIRED with the `"svelte"` FFI accepted string; the row-without-carrier is the structural source of the typed `UnsupportedLanguage` state.** The pairing rule restated structurally: an accepted string lands in the SAME change as its registry row; a registry row with no registered carrier behind it serves the typed known-but-unsupported state structurally (`host_executor` dispatch finds no carrier → `UnsupportedLanguage { adapter_id }`) — never a dangling kind, never a prose-only state. The `.svelte`-as-unknown-extension pin (`exact_resolution_tests.rs:655`) is superseded at B2 by the known-but-unsupported routing test; B8a registers the carrier and the typed-error path for `"svelte"` goes dead naturally; the LSP watcher glob widens to `.svelte` at B2 and is asserted inert (no virtual-file wiring until B8c). | Round-4 review (codex P1 + fable P3-1, one fix): B2 extended FFI accepted strings with `"svelte"` and tested unsupported behavior while the registry row was deferred to B8a — contradicting the same bullet's own "an accepted string without a registry row would be a dangling kind" rationale. The structural fix resolves both findings at once; the Angular doc's A2 pairing phrasing now reads as the same rule applied cross-program, not a stricter variant. |
| **D-ap** | **Svelte matrix round-4 completion (extends D-ad; same audited version 5.56.3 / docs date 2026-06-10): the docs-ToC Styling section and the Runtime `hydratable()` page are explicitly dispositioned, and the snippet projection ORDERING rule is pinned.** (a) Styling (docs `/scoped-styles`, `/global-styles`, `/custom-properties`, `/nested-style-elements`): scoped/global styles, `:global(...)` / `:global {}` blocks, `-global-` keyframes = CSS-domain with NO type-facing surface — the component `<style>` block stays an opaque recorded span (B8a parser), stripped from projection, no diagnostic; CSS custom-property attributes (`--track-color={expr}` on components/elements) = SUPPORTED (pass-through) — parsed as plain attributes, STRIPPED from the projected JSX attribute position (a `--`-prefixed name is not a valid JSX attribute identifier — verbatim projection would break TSX validity), `{expr}` values projected void-checked so hover/diagnostics inside the value survive, no diagnostic (stable, documented Svelte); nested `<style>` elements inside the template = parse-without-crash, opaque content, stripped from projection. (b) `hydratable(key, fn)` (Runtime docs, stable) = SUPPORTED (transparent) — an ordinary `svelte` import typed by the package; runtime JS API only, no template surface, zero adapter work. (c) Snippet ordering rule: ALL `{#snippet}` declarators of a projected scope function are emitted at the TOP of that scope function, in source order, before any sibling content, via CodeTransform MOVE operations (snippet bodies keep their original mapped spans) — Svelte scopes snippets to their whole lexical scope (a preceding sibling may `{@render}` a later-declared snippet), and in-place `const` projection would be a TS use-before-declaration error under the clean-type-check gate. | Round-4 review (codex P1 — the matrix claimed current-docs completeness while the docs ToC carries Styling and Hydratable-data pages it never dispositioned; fable P3-2 — the const-bound snippet declarator vs preceding-sibling visibility TDZ hazard). Both close inside the matrix's own conventions: explicit rows + exact landed behavior, byte-exact gate preserved. |
| **D-aq** | **Cache-family terminology pinned: the `FrameworkSurfaceDtoStore` is a CONTENT-ADDRESSED + fact-validated, generation-scoped memo — NOT a content-free query-identity cache; "content-free query identity" vocabulary is reserved for the semantic query caches.** The store's key DELIBERATELY carries the owner content hash (`owner_whole_hash` — exactly the live `VueMacroDtoKey.whole_hash`, `typeinfo/adapters/vue/store.rs:55`), placing it in the content-addressed family of the R-rules' two-family split; validation is value-side (`validated_at_generation` strict same-generation gate + `ReadSetSignature.facts`). "NO version column" in D-p/D-y means NO ADAPTER/NORMALIZER version column (process-lifetime memory; a normalizer change clears the store) — it never meant content-free. Content-free query-identity vocabulary applies to the `RouteDb` / `SemanticGraphStore` / `AngularTemplateScopeDb` class only. D-p/D-y/B5 wording adjusted accordingly. | Round-4 review (codex P2): "no version column" phrasing next to a whole-hash-bearing key muddied the two-family classification R6/R21 depend on; the live Vue store is explicitly content-addressed by `whole_hash`. |
| **D-ar** | **Angular-program round-4 reconciliations (recorded here for traceability; carried authoritatively by `docs/arch/angular-adapter-program.md`).** (i) The Angular doc is SELF-CONTAINED for execution: its §2 restates all twelve program-level invariants IN FULL and its §9 inlines every consumed substrate seam CONTRACT (routing + parse artifact, two-stage provider seam incl. the D-an exact-gate index, gated classification + per-file invalidation, DTO store incl. the D-aq family classification, per-kind status, virtual-file naming incl. the D-al testing role, synth seam, api-projector seam, tag semantics, stage-2 sub-key scoping, the D-aj reconciliations) — substrate D-* ids remain as traceability tags only, never load-bearing references. (ii) The go/no-go gains a REQUIRED pre-A0 fresh-docs audit criterion: recorded Angular version + docs scope (signal `input()`/`output()`/`model()`, template control flow, TCB semantics) + deltas vs the doc's designs; any delta updates the design before A0 executes. (iii) The `AngularTemplateScopeDb` build moves A1 → A2, landing WITH its only consumer (the TCB) — A1 stays facts + surface, so the legitimate partial GO (A1 without A2) ships no consumer-less query-identity DB; the guard `angular_template_scope_db_key_content_free` and the capability flip-on/flip-off tests move with it (A1 keeps the stage-2 fact-slot capability tests). | Round-4 review (codex P1 ×2 — a deferred program importing "all twelve substrate invariants" and D-* designs by reference is not executable cold; no fresh-docs revalidation gate for a program that runs later; fable P3-3 — A1 built a query-identity DB whose only consumer is A2's TCB). |
| **D-as** | **Adapter-visible ctx and executor-private resolution are SPLIT; adapters can express resolution demand ONLY as `PlannedDemand` data — no resolve method is reachable from adapter code.** SUPERSEDES the D-am op placement (and revises D-ai's "1:1 onto `FrameworkAdapterCtx` operations" phrasing): the four resolve ops `instantiate_public_type` / `resolve_macro_payload` / `project_path` / `shallow_surface` AND `export_graph` move OFF `FrameworkAdapterCtx` onto the EXECUTOR-PRIVATE resolve surface (`ExecutorResolveCtx`, module-private to the executor module `typeinfo/framework_surface/mod.rs` — never exported, never passed to adapter code); the executor consumes `PlannedDemand` and drives these ops itself, so `PlannedDemand` variants map 1:1 onto EXECUTOR ops (D-ai unchanged in substance). The FIRST registered adapter satisfies this structurally, not by exemption: B5 relocates the live Vue resolve delegates (`resolve_vue_public_type`, the `resolve_vue_macro_surface`/`vue_macro_dtos` pipeline) out of `typeinfo/adapters/vue/` into `typeinfo/framework_surface/vue_exec.rs`, and `VueFrameworkAdapter` is a TRUE plan/normalize impl holding no resolve entry point (D-ax). `FrameworkAdapterCtx` — what `plan_surfaces`/`normalize` receive — keeps EXACTLY TWO methods: `carrier_for::<T>(canonical)` + `script_facts_for::<T>(canonical)` (carrier metadata + validated facts; no semantic dispatch, no query-time walks). `normalize(ctx, resolved)` receives the executor's resolved results as DATA; an adapter needing more semantic data adds a new `PlannedDemand` variant (a compile-visible executor decision under D-ai's exhaustive match), NEVER a ctx op. D-am's raw-source ban list and enumeration discipline stand unchanged over the smaller surface. Guard `framework_adapter_ctx_closed_surface` re-specified (§6): (i) static-grep — `typeinfo/adapters/**` references NONE of the five resolve/export op names nor `ExecutorResolveCtx`; (ii) the API-surface pin enumerates the ctx's TWO public methods (no adapter-visible ctx exposes a semantic resolve method); (iii) the executor-private type is module-private and unreachable from adapter modules. | Round-5 review (codex P1): with the four resolve ops + `export_graph` on the adapter-visible ctx, `plan_surfaces`/`normalize` bodies could run arbitrary query-time semantic walks through the shared engine — "the executor owns resolution" held by convention only. Splitting the types makes the one-engine invariant structural: the only resolution currency an adapter holds is declarative `PlannedDemand` data. |
| **D-at** | **The Svelte api shim renders a type-only import/re-export PRELUDE for every preserved type reference (amends D-ak).** The D-ak Svelte declaration-shim renderer emits, ahead of the declarations, minimal TYPE-ONLY import/re-export lines derived from the carrier's SHALLOW script/module import facts — exactly the imports the preserved references need: named imports incl. aliases (`import type { Props as P } from './types'`), default imports, namespace imports, and the re-exports the shim surface forwards; rendered from the shallow inventory ONLY (covered by the same `api_projectors_render_shallow_no_resolution` static guard — no semantic dispatch, no OXC, no query-time resolution; tsserver resolves the emitted declaration text itself). Unused imports are dropped: the prelude is computed from the preserved-reference set, not a verbatim copy of the carrier's import list. Without this prelude a preserved ref to an imported alias is an unresolved name in `C.svelte.ts` — the shallow-by-default rule (refs preserved, never inlined) REQUIRES import emission as part of the shim contract. Tests: B8a snapshots assert the prelude lines (alias/default/namespace/re-export fixtures, each preserved ref's import present, no inlining); the B8c tsserver e2e asserts an imported `$props()` alias resolves THROUGH `C.svelte.ts` (no inlining, no semantic dispatch). | Round-5 review (codex P1 + fable P3-2, one fix): the shim spec preserved refs un-inlined but never said where their imports come from; the live Vue api producer has explicit external-type handling (`virtual_file_pipeline.rs:1536` vicinity) — the Svelte leg forbids semantic dispatch, so the syntax/shallow import prelude is the equivalent, contract-level mechanism. |
| **D-au** | **`ComponentDefaultSynth` consumes PARSE-DOMAIN inputs ONLY; stage-2 validated facts never enter synth (supersedes the D-n stage-2 hand-off).** `ComponentDefaultSynthCtx.script_facts: &FrameworkScriptFactSet` (stage-2 validated) is REPLACED by `script_candidates: &FrameworkScriptCandidateSet` — the per-file PARSE-DOMAIN stage-1 candidate collection; every other ctx field (`canonical_id`, `language`, `script_analysis`, `framework_parse`) is already parse-domain. Rationale (rail consistency, verified against the live tree): synth output is injected into `ShallowFileState` (`inject_vue_default_into_shallow_state` — `host_manage/prepared_decl.rs:1749`, `overlay_materialize.rs:534`), a CONTENT-DOMAIN artifact carrying NO `ReadSetSignature`, whose semantic consumers (the `Instantiate { canonical, "default" }` memo) version-root via the owner `FileWholeHash`. A content-addressed artifact must be a pure function of its key (content + parse env + `file_language_id` + parser/provider versions); a synth impl baking in a resolve-env-dependent stage-2 bit would leave a warm under-validated symbol after a stage-2 flip (package appears/disappears; capability flip) with NO content edit — no rail catches it. The live Vue synth already conforms (`snapshot.macros` is parse-domain); the Svelte synth shape (`{ $props: Props }` + exports) is derivable from stage-1 candidate fields; Angular registers no synth. Stage-2 facts stay QUERY-TIME-CONSUMER currency via `FrameworkAdapterCtx::script_facts_for` (fact-traced at query time). A future vertical whose synth genuinely needs resolved-symbol currency must FIRST rehome the synthesized symbol out of content-addressed shallow state into a fact-validated query cache — the shallow slot never stores stage-2-derived data. NEW guard `component_default_synth_parse_domain_only` (§6) + B8a behavioral test: the synthesized default symbol is structurally IDENTICAL across a stage-2 flip (fake-`svelte` fixture gaining/losing the real package; capability state) — DISCRIMINATING: a synth reading stage-2 facts would diverge. REJECTED: fact-tracing the synth-time stage-2 read into the embedding compute's `ReadSetSignature` — the shallow build is not a fact-traced compute, its consumers root by `FileWholeHash`, and threading fact observation through shallow-state construction would smear the two-family split for zero in-scope need. | Round-5 review (fable P2-1): the D-n hand-off gave synth resolve-env-dependent inputs while its output landed in content-keyed shallow state version-rooted by owner whole-hash only — a latent under-validation class the contract invited without a rail. Closed by constraining the seam to the cache family its storage lives in (decided directly — the live-tree mechanics made option (b) structurally inconsistent; no consult needed). |
| **D-av** | **`@verter/svelte-jsx` is a REAL workspace package at `packages/svelte-jsx/` — single content authority, host-embedded with a byte-pin, located via the provider `paths` injection; created in B8c (completes D-ae).** (a) HOME: NEW `packages/svelte-jsx/` — `jsx-runtime.d.ts`, `jsx-dev-runtime.d.ts` (re-export), `package.json` (types-only, no runtime JS; `exports` typing `./jsx-runtime` + `./jsx-dev-runtime`); published with the other `@verter/*` packages and declared a dependency of `@verter/typescript-plugin` (bundled into the VS Code extension with it). (b) EMBED + BYTE-PIN: the canonical hand-written source is `packages/svelte-jsx/{jsx-runtime.d.ts, jsx-dev-runtime.d.ts}`; `verter_session` carries IN-CRATE MIRROR files at `crates/verter_session/src/framework/svelte_jsx_assets/{jsx-runtime.d.ts, jsx-dev-runtime.d.ts}` and embeds THOSE at compile time via crate-relative `include_str!` — a literal cross-tree `include_str!("../../../packages/svelte-jsx/…")` is FORBIDDEN: it would embed the authority file itself (the freshness byte-compare degenerates to comparing a file with itself — vacuous) and breaks crates.io packaging (`cargo package` cannot include files outside the crate root). The freshness test (`crates/verter_session/tests/svelte_jsx_shim_freshness.rs`, the `typeinfo_proto_ts_freshness.rs` pattern) byte-compares each in-crate mirror against its `packages/svelte-jsx/` canonical — the package files are the single hand-written authority; drift fails the gate. (c) LOCATING per consumer: the provider `paths` injection point (`configure_paths`, `crates/verter_lsp/src/extension_provider.rs:924` — the LSP extension provider is the OWNER of provider path injection; the same function sets `jsxImportSource: "vue"` at `:932`) maps `@verter/svelte-jsx/jsx-runtime` + `jsx-dev-runtime` to the host-selected on-disk location — ts-plugin consumers (real tsconfig projects, VS Code) resolve the plugin-bundled package copy via normal node resolution relative to the plugin install; provider-inferred projects and TSGO (which reads REAL files — virtual content cannot serve it) resolve the host-MATERIALIZED embedded copy, written once per host version into the host's own data directory (NEVER into the user workspace); non-VS-Code LSP clients and the NAPI/tsserver-ipc consumers ride the same host-owned provider configuration. The host-selected copy is AUTHORITATIVE and version-matched to the projection the compiler emits — a workspace-installed `@verter/svelte-jsx` is NOT consulted (no version-drift class; deterministic resolution; npm publication exists for explicit user pinning, not as the resolution mechanism). **TRANSITIVE-DEPENDENCY resolution contract (D-ay):** the shim's OWN imports (`svelte`, `svelte/elements`, `svelte/attachments`) cannot resolve from the host-selected copy's location — a node_modules ancestor walk from the host data directory (or from the plugin install directory) never reaches the user workspace's `svelte` install, and `baseUrl` does not rescue node-style specifiers under `moduleResolution: "bundler"` — so the SAME `configure_paths` injection adds, alongside the shim rows, `paths` rows mapping `svelte` and `svelte/*` to the OWNER WORKSPACE's installed `svelte` package, resolved ONCE per owner project by the host through the existing workspace package resolution (per-owner-project rows — a monorepo with multiple `svelte` installs resolves each project against its own copy); when the owner workspace has NO `svelte` install, NO `svelte` rows are injected and D-ae(d)'s fail-closed behavior surfaces (module-not-found + the typed `svelte-package-missing` diagnostic). (d) HERMETIC FIXTURES: B8c gate fixtures `paths`-map `@verter/svelte-jsx` directly at the in-repo `packages/svelte-jsx/` (no npm install at test time); the B8c e2e includes the asset-resolution case — a fixture workspace with NO `@verter/svelte-jsx` npm dependency resolves the shim through the provider mapping — AND the D-ay PRODUCTION-TOPOLOGY case: shim materialized OUTSIDE the fixture workspace, `svelte` types present ONLY inside the workspace's own `node_modules` (vendored in-repo), no vendored `svelte` paths mapping — only the mechanism's injected rows make the shim's imports resolve. | Round-5 review (fable P2-2): the program's single most novel runtime asset had no repo home, no creating block, and no distribution/locating story across the LSP, ts-plugin, non-VS-Code, and ipc consumers — the B8c clean-type-check gate and the TSGO pragma-parity precondition both depend on the file existing at a resolvable path. |
| **D-aw** | **Angular-program round-5 reconciliations (recorded here for traceability; carried authoritatively by `docs/arch/angular-adapter-program.md`).** (i) `AngularTemplateScopeKey` DROPS `type_env_hash` + `lib_env_hash` — final key `{ owner_component_slot, consumed_capability_bits, project_identity, resolve_env_hash }`: the value (selector index, pipe NAME map, schemas, scope diagnostics) is built from validated stage-2 facts + structural import/declaration resolution — symbol-identity work in the resolve-env domain; pipe `transform` SIGNATURES resolve in the TCB through the shared semantic queries (which carry their own type/lib dims) and are NOT stored in the scope DB. R21 scoping: a dim enters a key only when the value depends on it; if a future scope-DB value genuinely stores type-meaning- or lib-dependent data, the dependence is recorded and the dim added THEN (the D-ah pattern). (ii) A0 registers the EXPLICIT DEFERRED TAG ROW for `FRAMEWORK_TAG_ANGULAR` in the session registry's tag mapping in the SAME change as the proto value — `framework_registry_complete` (every wire tag maps to a registered adapter OR an explicit deferred/out-of-scope TAG row) is otherwise structurally red at A0's own gate, since the real registration arrives only in A1 (which supersedes the row); the Angular doc's §9 now inlines the tag-completeness clause so a cold executor holds the rule in-doc. (iii) The Angular doc's §1.2 substrate summary no longer reads as substrate scope: the `.html` gated-candidate row and the first capability bit are stated to land in the ANGULAR program (bit: A1; row: A2) — the substrate ships only the empty-snapshot mechanism. | Round-5 review (codex P2 — scope-DB over-keying vs R21; fable P1-1 — A0 structurally failing its own gate with no in-doc resolution; codex P3 — §1.2 scope ambiguity). Each fix follows the substrate's own rails: D-ah's scoped-dim discipline, D-ag/D-aa's explicit-row vocabulary, D-ar's self-containedness bar. |
| **D-ax** | **B5 RELOCATES the Vue resolution delegates out of `typeinfo/adapters/vue/` into the executor module; `VueFrameworkAdapter` is a TRUE plan/normalize impl (completes D-q's "keeps ONLY" promise; makes D-as structural for the first registered adapter).** Ground truth: the live `typeinfo/adapters/vue/` directory IS the Vue resolution machinery — `public_type.rs:56 resolve_vue_public_type` (`ensure_indexed_ready` :90, `ProjectSemanticDispatch` :103, `SemanticQueryKey::Instantiate` :109) and `surface.rs:198 resolve_vue_macro_surface` (`ctx.ensure_indexed_ready` :235/:615/:867, `IndexedReady.raw_source` :436-437) sit INSIDE the directory both B5 guards scan. Disposition (per-file table in B5): `public_type.rs` and `surface.rs` RELOCATE wholesale to NEW `typeinfo/framework_surface/vue_exec.rs` (the executor becomes the directory module `typeinfo/framework_surface/` — `mod.rs` owns the executor + the module-private `ExecutorResolveCtx`); the three `*_from_typeinfo_surface` DTO producers relocate WITH the pipeline (they take `&dyn ResolverContext`, raise member values via `ctx.dispatch()`, and slice JSDoc from cache-owned raw source — RESOLUTION legs, not normalizers); `runtime_ctor.rs` STAYS (pure `TypeExpr` walk, zero banned tokens); `store.rs` retires per D-p; `mod.rs` is rewritten with NO re-export shim of any relocated name; NEW `adapter.rs` holds the plan/normalize impl — `plan_surfaces` emits `PlannedDemand::PublicTypeInstance` + per-kind `PlannedDemand::MacroPayload`, the executor's private ops delegate to the relocated byte-pinned functions, `normalize` consumes resolved results as data. Every production caller re-imports from `vue_exec` (call-site list in B5); the retained `impl VerterHost` methods and the executor ops CONVERGE on the same relocated delegates — one semantic path. Vue behavior is byte-identical (module re-homing + interface conformance, no semantic change): the existing Vue suites + `framework_surface_vue_roundtrip` pin it; the `framework_adapter_ctx_closed_surface` static half is RED pre-relocation, green only on the relocated tree. | Round-6 review (fable P0-1): the `framework_adapter_ctx_closed_surface` parenthetical asserted the Vue delegates "live OUTSIDE `typeinfo/adapters/`" — verified FALSE on the live tree; no block relocated them, so both B5 guards were structurally red on B5's own landed tree, and the Legacy-Deletions note "the adapter calls the same functions" gave the first registered adapter direct resolve entry points — exactly what D-as declares impossible. Same defect class the program graded P0 in round 3 (a block that cannot pass its own gate as written). The named-allowlist alternative was REJECTED: inconsistent with the "by construction, not by exemption" bar and with D-q. |
| **D-ay** | **The `@verter/svelte-jsx` shim's TRANSITIVE dependencies resolve through injected per-owner-project `svelte` paths rows (extends D-av(c)); the B8c gate gains a production-topology fixture.** The shim's own imports (`svelte`, `svelte/elements`, `svelte/attachments`) are non-relative; the host-selected shim copy lives OUTSIDE the user workspace (host data directory for provider-inferred/TSGO consumers; plugin install directory for the ts-plugin bundle), so the node_modules ancestor walk from the shim's directory never reaches the workspace's `svelte` install, and `baseUrl` does not rescue node-style specifiers under `moduleResolution: "bundler"`. Contract: the SAME `configure_paths` injection (`crates/verter_lsp/src/extension_provider.rs:924`) adds, alongside the shim rows, `paths` rows mapping `svelte` and `svelte/*` to the OWNER WORKSPACE's installed `svelte` package — resolved ONCE per owner project by the host through the existing workspace package resolution; rows are PER OWNER PROJECT (multi-`svelte` monorepos resolve each project against its own copy); a workspace with NO `svelte` install gets NO injected rows and fails CLOSED per D-ae(d) (module-not-found + `svelte-package-missing`). Gate addition (B8c Tests 3): a PRODUCTION-TOPOLOGY fixture — shim materialized outside the fixture workspace, `svelte` declarations only inside the workspace's own `node_modules` (vendored, hermetic), no `svelte` mapping beyond the injected rows — type-checks CLEAN, and removing the injected rows fails it. | Round-6 review (fable P1-1): every named B8c fixture vendors `svelte` and `paths`-maps it project-wide, which incidentally resolves the shim's own imports — fixture-green/production-red on the flagship clean-type-check gate, the direct residual of the round-5 D-av finding one transitive hop further out. |
| **D-az** | **The session-side blessed downcast wrapper lands in B4: `crates/verter_session/src/framework/ctx.rs` opens with ONLY the bare token-gated `carrier_for::<T>(artifact, &CarrierAccessToken)` free helper (+ `Arc` form); B5 extends the same module with `FrameworkAdapterCtx`, whose `carrier_for` method routes through the same helper (amends D-m/B4).** B4's `vue_parse()` accessor calls this helper — never the raw `__carrier_downcast_ref` — so the `carrier_downcast_confined_to_owning_adapter` three-file allowlist (`verter_language::parse_artifact`, `verter_session/src/framework/ctx.rs`, `verter_compiler/src/framework_common/ctx.rs`) is satisfied on B4's own landed tree with no allowlist churn and no temporary exemption; the guard's final-state text is true from B4 onward. | Round-6 review (fable P2-1): as written, `framework/ctx.rs` was a B5 deliverable, so B4's `vue_parse()` had to call the raw downcast directly — tripping B4's own guard allowlist (the same cannot-pass-its-own-gate class as P0-1, one block earlier). Landing the bare helper in B4 is the no-churn option fable's review prescribed as cleaner. |
| **D-ba** | **`verter_language` is the SOLE `CarrierAccessToken` minting authority; descriptors RECEIVE, never construct (amends D-m).** The token is created in exactly one place: inside `verter_language`, by a named token factory private to the crate, during `LanguageRegistry` carrier-row construction. The minted token is returned exactly ONCE, to the registry-construction caller, as the carrier row's registration proof; `FrameworkAdapterDescriptor` construction (B5) and `vue_parse()` (B4) RECEIVE/reuse that token. The `_private: ()` non-public field keeps out-of-crate struct literals uncompilable; NO public arbitrary-id constructor (`new(adapter_id)`/`From`/`Default`) and NO public by-id token lookup exist (API-surface pin — a by-id lookup would let any crate fetch any adapter's token, the same forging vector as a public constructor). NEW guard `carrier_access_token_minted_only_in_verter_language` (B4, §6): `CarrierAccessToken` struct-literal/constructor expressions appear ONLY in the owning `verter_language` minting files (`parse_artifact.rs` + the `LanguageRegistry` row-construction module) and explicit test fixtures. | codex P1: D-m named TWO minting authorities — `LanguageRegistry` rows AND `FrameworkAdapterRegistry` descriptor construction — which is unenforceable either way: a private field makes `verter_session`'s descriptor-side construction uncompilable, while a public constructor lets any crate forge any adapter's token. ONE in-crate authority + received-proof descriptors makes the privacy mechanically real; the guard + API-surface pin make it durable. |
| **D-bb** | **The D-af sweep covers `parse_sfc` PRODUCER CALL SITES, not only `ParsedSfc` type references; the confinement guard scans BOTH tokens (amends D-af).** Live-tree producer sweep (exhaustive over `verter_session` production code): `parse.rs:105` (inside `parse_vue_snapshot`, already a CONFINE row); the on-demand re-parse builders `build_script_analysis_from_source` / `build_style_analyses_from_source` (`parse.rs:752/:769`, `parse_sfc` at `:755/:773`; callers `host_manage/analysis_io.rs:424/:435`); the template-analysis cold/merged-source re-parses (`host_manage/analysis_io.rs:93/:100` in `build_template_analysis`, `:237/:244` in `compute_template_analysis_if_missing`, plus its `parse_vue_snapshot` call at `:185`); and the route-owned cold-parse producer (`host_resolve/route_owned_shallow.rs:303-306` — direct `parse_sfc` gated by `ends_with(".vue")`, local named `cached_parse`; the producing twin of the tabled `overlay_materialize.rs:354` row). B4's disposition table carries the rows (REPLACE for the route-owned local; CONFINE for the Vue builder halves — the `analysis_io.rs` halves RELOCATE behind the Vue bridge so `host_manage/**` ends `parse_sfc`-free as well as `ParsedSfc`-free); `parsed_sfc_confined_to_vue_bridge` scans BOTH the `ParsedSfc` type token AND the `parse_sfc` call token against the same allowlist. | fable P2-1: the route-owned cold-parse producer was missing from the D-af table, and a type-inferred `let parsed = parse_sfc(…)` call carries no `ParsedSfc` token — a type-reference-only scan could not catch a misplaced producer, leaving a hole exactly where the sweep's exhaustiveness claim mattered. |
| **D-bc** | **The Svelte DTO-store key remainder is `{ source: SvelteSurfaceSource }` — the CLOSED source-family discriminant `SvelteSurfaceSource { RunesProps, LegacyExportLet, Bindable, SnippetProps, LegacySlotInventory, LegacyDispatcher, InstanceExports }` (completes D-y for B8b).** The typed `Eq + Hash` adapter remainder parallel to Vue's `{ macro_index, macro_kind }`: Vue needs an index because one SFC carries several macro sites; a Svelte component has at most ONE declaration site per source family (derived from the §9 mapping: `$props()` incl. snippet-typed, legacy `export let`, `$bindable()`, legacy `<slot>` inventory, legacy `createEventDispatcher<E>`, exported instance members), so the family discriminant alone is the minimal structural remainder. Kinds composed from two families (SLOTS = snippet-typed props + legacy `<slot>` inventory) occupy two source rows merged at normalise time — each cached bundle stays single-source and collision-free. Pinned by extending the `framework_surface_store_key_structural` whole-struct destructure test with the Svelte row in B8b. | fable P3-3: B8b consumed the generic store without pinning its adapter remainder — the one adapter-designed key column was unspecified for the program's flagship vertical, inviting an ad-hoc (or digest-shaped) key at implementation time. |
| **D-bd** | **`FileLanguage::FrameworkTemplate` rows carry NO carrier-compiler obligation; `carrier_descriptors_have_compilers` binds `carrier_language: Some(_)` descriptors only (recorded here for traceability; carried authoritatively by `docs/arch/angular-adapter-program.md`).** "Carrier-bearing" in the guard means the descriptor's singular `carrier_language` column is populated; a `FrameworkTemplate` language never populates it. A template file is OWNER-ROUTED: it is consumed by the owning component's build (the Angular TCB sidecar is produced by `crates/verter_compiler/src/angular/ide/` dispatched off the OWNING COMPONENT through the sidecar virtual-file pipeline), never independently compiled — a registered standalone template compiler would be a second entry path into template compilation. The Angular descriptor's `carrier_language` is `None` (components are real `.ts` Script files), so the guard imposes no compiler obligation on the Angular row; the extracted program's A2 re-run asserts the guard stays green across the Angular registration and that NO `CarrierCompiler` row exists for `"angular"`. REJECTED: a typed `template_language` descriptor slot — it would create a compiler-registry obligation with no dispatch consumer (nothing compiles a `FrameworkTemplate` file standalone). | fable P3-5: the Angular doc asserted "the Angular template carrier row has its registered compiler" while the descriptor field is singular `carrier_language` and `.html` is not a carrier — the obligation was unimplementable as written; the owner-routed exemption is the structurally consistent reading. |

---

## 4. Risk Register

| # | Risk | L/I | Mitigation |
|---|---|---|---|
| R1 | (EXTRACTED — risk transfers to `docs/arch/angular-adapter-program.md`) Angular template type-check depth (microsyntax, pipes, DI context, host directives, custom structural-directive guards) balloons | High/High | Angular is extracted behind its own go/no-go (D-ac); the mitigation design (selector scope via `AngularTemplateScopeDb`, `null!`-instantiation TCB, strict-template semantics, out-of-scope rows vs ledger rows) is carried in full by the extracted program doc |
| R2 | Svelte 5 runes/snippets semantics drift + Svelte 4 legacy breadth | Med/High | Pin to the audited Svelte version (5.56.x, D-ad), runes-first with event ATTRIBUTES primary (D-u); legacy coverage = `export let` props + dispatcher emits + `on:` projection + `<slot>` inventory in separate fixtures; explicit out-of-scope rows in the B8a current-docs matrix (D-ad — supersedes D-u's shorter enumeration); compiler-version-pinned vendored fixtures; a Svelte version refresh re-runs the docs audit before scope claims change |
| R3 | Second-resolver creep in per-framework normalizers (the historical hang/divergence class) | Med/Critical | Executor-owned resolution (D-f plan/normalize split, made structural by D-as — adapters hold no resolve method at all, only declarative `PlannedDemand` data); `FrameworkAdapterCtx` capability surface (no OXC, no raw source, no dispatch handle, no resolve op by construction); guards `framework_adapters_are_thin_no_second_resolver` + `framework_adapter_ctx_closed_surface`; dual review on every adapter block |
| R4 | Parse-carrier cutover blast radius — SEVEN production `cached_parse` struct carriers + the `Option<&ParsedSfc>` threading APIs (D-af sweep) | Med/Med | The B4 disposition table is exhaustive over the live-tree sweep (REPLACE / CONFINE / RENAME per site); confined typed accessor (`vue_parse()`); byte-identity characterization suite pinning `HostSourceData.source_type`, overrides, eval-source building, route-owned shallow; retired-symbol gate on the `cached_parse` token + static guard `parsed_sfc_confined_to_vue_bridge` |
| R5 | Hot-path perf regression from registry indirection on the Vue path | Low/High | Interned-id table lookup in per-file loops; `dyn` only at the per-request typeinfo boundary; `canary_warm_hit_zero_alloc` + `perf_bounds/` as gates; before/after bench in B4/B5/B6 |
| R6 | Wire schema bump breaking out-of-tree clients | Low/Med | `SUPPORTED = [2, 3]`; tags add-only at next free numbers; reserved-tag discipline; byte-pinned TS regen; the `UnknownSchemaVersion` error payload carries `server_supported_versions` (the advertisement surface, populated from the supported-set constant) |
| R7 | (DEFERRED — rides with B9) Astro cross-framework islands (`.astro` importing `.vue` + `.svelte`) exercising recursive cross-adapter resolution | Med/Med | All public types resolve through the ONE `Instantiate` identity (framework-blind, keyed by canonical); explicit island fixture matrix in the deferred B9 design; recursion terminates by query identity (the `.vue`-imports-`.vue` discipline, already proven; `.svelte ↔ .svelte` re-proven in B8a) |
| R8 | (EXTRACTED — risk transfers to `docs/arch/angular-adapter-program.md`) LSP sidecar virtual files (Angular TCB) destabilizing `sync_coordinator` | Med/Med | Sidecar work lands only in the extracted Angular program, after the single-virtual-file path is proven for Svelte (B8c) — the proven-first ordering is a go/no-go criterion there |
| R9 | Editor ecosystem conflicts with official Svelte/Astro extensions | Med/Low | Attach to existing language ids, contribute no grammars; opt-in `verter.frameworks` workspace setting |
| R10 | Known-bug accumulation silently rotting (core fixes forbidden in-program) | Med/Med | Typed ledger with discriminating red bodies + bijection guard; B12 exit report enumerates every row as the follow-up semantic program's input |
| R11 | (DEFERRED — rides with B7) React HOC/`forwardRef`/`memo` inference explosion | Med/Med | The deferred B7 design's step-0 probe bounds the v1 scope (D-t); wrappers/`FC<P>`/factory inference are known-bug ledger rows unless probe-proven; typed unsupported diagnostics instead of heuristics; EXPOSE out of v1 |
| R12 | B3 rehoming blast radius (`resolve_type` 2k+ LOC, parser-arena coupling) | Med/Med | Pure physical move + rename with zero behavior change (full suite is the pin); the `bindings.rs` neutral/Vue split is the only surgical edit; guard written first (red), move turns it green |

---

## 5. TDD + Known-Bug Convention

**TDD is mandatory for every block**: write the failing tests first (each block's Tests section is
ordered failing-first), verify red, implement, verify green, refactor. Characterization tests must
discriminate (fail pre-change / pass post-change). Stub Prevention applies in full: no empty test
bodies, no always-true assertions, no "deferred body" placeholders on landed commits.

**Known-bug convention** (constraint: pre-existing semantic/typeinfo gaps surfaced by new
frameworks are filed, not fixed here):

- Source attribute (exact form):
  `#[ignore = "known-bug: <slug>; layer=<semantic|typeinfo|compiler-shared>; owner=<skill>; unblock=<one-line direction>"]`
  with a REAL discriminating body that runs red under `-- --ignored` today and green after the
  future core fix. Empty/trivial bodies are forbidden.
- Typed manifest: `crates/verter_session/tests/framework_known_bug_manifest.rs` (cloned from the
  `typeinfo_ignored_test_manifest.rs` pattern):

  ```rust
  pub struct KnownBugRow {
      pub slug: &'static str,
      pub file: &'static str,
      pub function: &'static str,
      pub layer: PreexistingLayer,     // Semantic | Typeinfo | CompilerShared
      pub framework: &'static str,     // adapter id that surfaced it
      pub discriminates: &'static str, // why it fails pre-fix and passes post-fix
      pub unblocker: &'static str,     // one-line future-fix direction
  }
  ```

- Guard `framework_known_bug_ledger_bijection`: bijection between `known-bug:`-prefixed ignores in
  framework test files and manifest rows; count exactness; static scan rejecting trivial bodies.
- **Ledger scope (re-scope, D-ab)**: in this program, ledger rows are produced only by the
  IN-SCOPE surfaces — the Vue substrate parity work (B5/B6) and the Svelte vertical (B8a/b/c). The
  `framework` manifest column therefore takes only `"vue"` / `"svelte"` values here; deferred
  (React/Astro) and extracted (Angular) verticals adopt the same attribute + manifest + bijection
  convention in their own execution, appending to the same manifest file.
- B12's exit report enumerates the full ledger as the input to the follow-up semantic program.

---

## 6. Architecture Guard List (all new guards)

| Guard | Block | Kind | What it scans / asserts |
|---|---|---|---|
| `single_language_classifier` | B2 (widened B12) | static-grep | `ends_with(".vue")` / `".svelte"` / `".astro"` literals permitted only in `verter_language` + a named frozen-Vue allowlist (Vue compiler paths, Vue adapter); asserts exactly ONE language-kind definition in the workspace (no `FileKind` re-introduction; retired symbols `FileKind`, `ExportGraphFileKind` in the retired-symbol gate) |
| `ffi_no_silent_vue_default` | B2 | static + test | no `unwrap_or("vue")` in `verter_ffi`; absent kind classifies via `LanguageRegistry::classify_static(canonical_path)` (the pure leaf entry — FFI-time classification NEVER consults `ProjectCapabilitySnapshot`, so gated rows like `.html` are unreachable by inference and REQUIRE an explicit kind string at the FFI boundary, typed error otherwise) or returns a typed error when no path |
| `neutral_script_analysis_not_under_vue_path` | B3 | static-grep | production code neither defines nor imports framework-neutral symbols (the B3 relocation table's `script/type_surface` rows: `AnalyzedExternalTypeSource`, `RawSourceSurface`, `SymbolSpace`, `build_type_context`, `resolve_type_elements`, `infer_runtime_type`, `capture_statement_surfaces`, …) under/from EITHER legacy path form — `utils::oxc::vue::script::*` AND `utils::oxc::vue::resolve_type::*` (both the direct `verter_parser` path and the `verter_compiler::utils` re-export spelling) |
| `carrier_downcast_confined_to_owning_adapter` | B4 | static-grep | the raw `#[doc(hidden)] __carrier_downcast_ref` appears ONLY in `verter_language::parse_artifact`, `verter_session/src/framework/ctx.rs` (the bare token-gated `carrier_for::<T>` free helper — lands IN B4, D-az, so `vue_parse()` never calls the raw downcast and the B4 tree passes this allowlist; B5 extends the module with `FrameworkAdapterCtx`), and `verter_compiler/src/framework_common/ctx.rs` (the two blessed `carrier_for::<T>` wrapper homes); typed carrier use only inside the owning adapter/compiler paths — `typeinfo/adapters/<framework>/`, `verter_compiler/src/<framework>/`, AND `verter_compiler/src/framework_common/vue_bridge.rs` (Vue's compiler home); every downcast passes a `CarrierAccessToken` carrying the adapter's own registered id (D-m) |
| `carrier_access_token_minted_only_in_verter_language` | B4 | static-grep + API-surface test | `CarrierAccessToken` struct-literal/constructor expressions appear ONLY in the owning `verter_language` minting files (`parse_artifact.rs` + the `LanguageRegistry` row-construction module) and explicit test fixtures (D-ba — `verter_language` is the SOLE minting authority; the token is minted during `LanguageRegistry` carrier-row construction and returned exactly once to the registry-construction caller as the carrier row's registration proof); the API-surface half pins that NO public arbitrary-id constructor (`new(adapter_id)`/`From`/`Default`) and NO public by-id token lookup exist — descriptors and `vue_parse()` RECEIVE the token, never construct it |
| `framework_adapters_are_thin_no_second_resolver` | B5 | static-grep | Two scopes (statically distinguishable — no "query time" heuristic needed): (i) ABSOLUTE ban in `typeinfo/adapters/**` + `verter_session/src/framework/**` — no `parse_type_annotation`, no `oxc_parser::`, no `lower_ts_type`, no synthesize-then-reparse `format!` patterns; adapter code has NO resolution entry point at all — resolution demand is expressed only as `PlannedDemand` data consumed by the executor (D-as); for Vue this holds BY CONSTRUCTION because B5 relocates the Vue resolution delegates out of `typeinfo/adapters/vue/` into the executor module (D-ax) — the vue dir keeps only the plan/normalize adapter, the pure typed-IR `runtime_ctor.rs` normalizer, and the `vue_parse()` carrier accessor. (ii) ABSOLUTE ban on `lower_ts_type`/`oxc_parser::`/`parse_type_annotation` in `verter_compiler/src/{svelte,react,astro,angular}/**` too — per D-o, ALL shallow-time type lowering lives in `verter_semantic::analysis::framework_facts/**` (the one OXC pass), so `verter_compiler` framework dirs hold only carrier parsers (byte/template tokenizers) + `ide/` projections and never lower types; `verter_compiler` keeps ZERO `verter_type_expr_oxc` dependency |
| `framework_adapter_ctx_closed_surface` | B5 | static + API-surface test | THREE halves (D-am/D-as): (i) static-grep — `typeinfo/adapters/**` (every framework's surface-adapter impl) references NONE of the executor-private resolve/export ops (`instantiate_public_type`, `resolve_macro_payload`, `project_path`, `shallow_surface`, `export_graph`, `ExecutorResolveCtx`) NOR `ensure_indexed_ready`, `IndexedReady`, `raw_source`, `eval_source`, `HostSourceData`, `EffectiveFileState`, `StoreView` (the raw/eval/content-snapshot class; the live Vue resolution delegates — `resolve_vue_public_type`, the `resolve_vue_macro_surface`/`vue_macro_dtos` pipeline and its DTO producers — sit INSIDE `typeinfo/adapters/vue/{public_type.rs, surface.rs}` today and are RELOCATED by this same block into the executor module `typeinfo/framework_surface/vue_exec.rs` (D-ax), so the scan scope is clean by construction, not by allowlist; the static half is RED on the pre-relocation tree — discriminating for the relocation itself); (ii) an API-surface pin test enumerating `FrameworkAdapterCtx`'s public methods against the blessed D-as list (EXACTLY `carrier_for` + `script_facts_for` — NO adapter-visible ctx exposes a semantic resolve method) — adding an op fails the pin until D-as's enumeration is consciously extended; (iii) the executor-private resolve surface is module-private to the executor module (`typeinfo/framework_surface/mod.rs`; not exported; unreachable from adapter modules — asserted by the grep half) |
| `api_projectors_render_shallow_no_resolution` | B5 (re-run B8a) | static-grep | NON-Vue `framework/api_projectors/**` legs reference NO semantic dispatch (`ProjectSemanticDispatch`, `SemanticQueryKey`, `execute_cooperative`), NO OXC (`oxc_parser::`, `lower_ts_type`, `parse_type_annotation`), and NO query-time resolution entry points — the api shim is a pure declaration render over the shallow inventory + synth output (D-ak); the Vue leg is exempted by path (it delegates to the byte-pinned legacy extraction, which owns its own session-internal resolver context) |
| `framework_registry_complete` | B5 | runtime test | TWO distinguished registration legs (D-ag): every registered adapter id has a descriptor + a surface leg that is EITHER a registered surface adapter OR the explicit structural `SurfaceRegistration::Deferred` arm (served by the executor as per-kind `UNSUPPORTED` + diagnostic — never prose-only "unsupported"); every `FrameworkTag` wire variant maps to a registered adapter OR an explicit registered out-of-scope/deferred TAG row. With D-aa the live variant set is `NONE`/`VUE`/`SVELTE`/`REACT`/`SOLID`/`OPEN_CANONICAL`: at B5, `VUE` is the registered adapter (surface leg = `Adapter`); `SVELTE` carries an explicit deferred TAG row until B8a registers the adapter id with carrier legs + `surface: Deferred`, and B8b flips the arm to the real `SvelteFrameworkAdapter`; `REACT`/`SOLID` carry explicit deferred/out-of-scope TAG rows for the whole program (the de-facto proof of D-aa's tag-semantics rule); `NONE`/`OPEN_CANONICAL` are non-adapter sentinels asserted as such. The api-projector leg (D-ak): every descriptor with `virtual_file_naming.api_suffix = Some(_)` has a registered `ComponentApiProjector` leg when its carrier legs register (Vue at B5; Svelte at B8a). The compiler leg is deliberately NOT here (B5 must not depend on B6's `CarrierCompiler`) — see `carrier_descriptors_have_compilers` |
| `carrier_descriptors_have_compilers` | B6 (re-run per vertical — next run lands with B8a, the Svelte row; deferred/extracted verticals re-run it when they reopen) | runtime test | every carrier-bearing descriptor in the session registry has a registered `CarrierCompiler` in the compiler registry (Vue via `vue_bridge` from B6 on; each landed vertical adds its row) — "carrier-bearing" means the descriptor's singular `carrier_language` column is `Some(_)`; a `FileLanguage::FrameworkTemplate` language NEVER populates `carrier_language` and carries NO compiler obligation (D-bd: template files are owner-routed — consumed by the owning component's build, never independently compiled) — the compiler-completeness leg split out of `framework_registry_complete` so B5 ∥ B6 stays real |
| `framework_surface_wire_executor_validates_first` | B5 | test | executor rejects unknown adapter id / bad schema version through typed errors BEFORE any semantic dispatch (extends the `typeinfo_request_validation.rs` closed-set tests) |
| `framework_codegen_uses_code_transform` | B6 | static-grep | no `String::replace`/manual splicing on `build_string()` output in framework compiler modules; all mutation through `CodeTransform` ops |
| `script_fact_providers_zero_cost_on_miss` | B5 | perf/runtime test | the `ActiveProviderIndex` (built ONCE per registry construction / capability-snapshot change) is TWO exact-match maps over the closed `ScriptFactSyntaxGate` vocabulary — `by_carrier_language` + `by_import_specifier` (D-an; NO per-`FileLanguage::Script` provider list exists) — and the per-file active set is computed EMPTY pre-invocation: (i) with no specifier-gated provider registered, a `Script` file's shallow pass takes the `is_empty()` fast path — not even per-specifier lookups run; (ii) with a specifier-gated provider registered (in-tree fixture provider gated on a fixture specifier — the extracted Angular program's `"@angular/core"` row is the production stress case), a `Script` file lacking the exact specifier computes an EMPTY set via exact-key misses — the provider dispatch loop is NOT entered, ZERO `ScriptFactProvider` invocations, ZERO gate evaluations, ZERO fact-container allocations (the `framework_script_candidates`/facts slot stays `None`, no empty `FrameworkScriptFactSet` is materialized), no per-file capability checks beyond the existing interned classification lookup; (iii) the empty-set path is byte-identical to the pre-existing shallow code path. Re-asserted with EACH production provider registration — in this program with the Svelte provider in B8a; deferred/extracted verticals re-assert it with their providers when they execute. Perf coverage: a `perf_bounds/` bench with all in-scope adapters compiled in + registered against (i) a pure-TS project and (ii) a Vue project, exercising warm hits — non-framework shallow+warm paths show no regression vs the pre-program baseline |
| `framework_surface_store_key_structural` | B5 | static + unit test | the surface-store key path contains NO fixed-width digest used as a key component (no `u64`/`Hash16` adapter-key field — the adapter remainder is a typed `Eq + Hash` struct in a per-adapter sub-map); `query_level` is a first-class generic column; key structs carry no env-hash dims and no version columns (D-p); a whole-struct destructure unit test (no `..`) forces a conscious decision on any added key field — the live `VueMacroDtoKey` test pattern, generalized |
| `script_fact_capture_is_syntax_only` | B5 | static-grep | `verter_semantic/src/analysis/framework_facts/**` (stage-1 capture code) references NO import-resolution, route-fact, capability-snapshot, or `verter_session` types — its only host-derived input is the resolved `FileLanguage` row; resolved-symbol validation types appear ONLY in the session-side stage-2 module (`framework/script_facts.rs`) (D-o two-stage boundary) |
| `component_default_synth_parse_domain_only` | B5 (behavioral half re-run B8a) | static + test | `framework/synth.rs` + every `ComponentDefaultSynth` impl reference NO stage-2 types (`FrameworkScriptFacts`/`FrameworkScriptFactSet`) and no import-resolution/route-fact/capability-snapshot types; `ComponentDefaultSynthCtx` carries no stage-2 field (whole-struct destructure pin, no `..`); behavioral half (B8a): the synthesized default symbol is structurally IDENTICAL across a stage-2 flip (real vs fake `svelte` package; capability state) — synth output is a pure function of parse-domain inputs (D-au) |
| `parsed_sfc_confined_to_vue_bridge` | B4 | static-grep | production references to BOTH tokens — the `ParsedSfc` type AND the `parse_sfc` producer function (D-bb: a type-inferred `let parsed = parse_sfc(…)` call carries no `ParsedSfc` token and would escape a type-reference-only scan) — appear ONLY in the Vue-owned bridge allowlist: `crates/verter_parser/**`, `crates/verter_compiler/**` (the Vue compiler home incl. `framework_common/vue_bridge.rs`), and the named `verter_session` Vue-bridge files — `parse.rs` (the Vue producer/builder half), `host_resolve/vue_script_extract.rs` (B4 relocates the two Vue-semantic `eval_program.rs` functions there so `host_manage/**` ends `ParsedSfc`-free), and `typeinfo/adapters/vue/**` (the live session Vue adapter module — home of the `vue_parse()` accessor). Everything else reaches Vue parse data through the blessed `vue_parse()` / `carrier_for::<VueParseCarrier>` accessors. The B4 disposition table (D-af) is the per-symbol authority; this guard is the coarse rail |
| `angular_template_scope_db_key_content_free` | EXTRACTED (D-ac) | static + test | carried by `docs/arch/angular-adapter-program.md` — lands with the Angular TCB block (A2) there, together with the scope DB and its consumer (D-ar(iii)); not built in this program |
| `framework_known_bug_ledger_bijection` | B6 | manifest guard | §5 |
| `react_callback_props_not_emits` | DEFERRED (rides with B7, D-ab) | test | React adapter never classifies callback props as EMITS — lands when B7 reopens |
| (retired-symbol gate additions) | B2/B4/B5 | static-grep | `cached_parse` (the token, workspace-wide in production — ALL SEVEN carrier fields of the D-af sweep are renamed/replaced, and the `route_owned_snapshot_cached_parse_hits` counter family is renamed, so the token retires cleanly), the four `FileKind` enums, `ExportGraphFileKind`, `ffi_file_kind_to_host` (B2/B4), `VueShallowMetadataStore` + `VueMacroDtoKey`-as-store-key (B5, migrated onto `FrameworkSurfaceDtoStore` per D-p) added to the `no_legacy_walker.rs`-pattern retired list |
| (existing pins re-run) | B1+ | — | proto/TS taxonomy parity, byte-equal TS bindings, audit parity, request-validation closed-set — all green across the schema bump |

**New CRITICAL rule** ("Framework Adapters Are Thin Projections (CRITICAL)") is added to CLAUDE.md
+ a new `/framework-adapters` skill in B5, registered in `CRITICAL_RULE_GUARDS` with
`framework_adapters_are_thin_no_second_resolver` + `framework_registry_complete` in the SAME change
(R6 meta-guard satisfied).

---

## 7. Block Plan Overview

**Execution scope (D-ab)** — these blocks, and ONLY these, execute in this program:

| # | Block | Depends on | Parallel-safe with |
|---|---|---|---|
| B1 | Wire completion (the one wire block) | — | B2, B3 |
| B2 | `verter_language` + routing cutover | — | B1 |
| B3 | Neutral script-analysis rehoming (FQ1) | — (scheduled after B2: same-crate file overlap) | B1 |
| B4 | `FrameworkParseArtifact` carrier cutover (FQ2) | B2, B3 | — |
| B5 | Adapter registry + framework-surface executor + Vue re-housing | B1, B2, B4 | B6 |
| B6 | Compiler framework scaffold | B2, B4 | B5 |
| B8a | Svelte parser + shallow + synth | B5, B6 | — |
| B8b | Svelte typeinfo adapter | B5, B8a | B8c |
| B8c | Svelte IDE TSX + LSP | B8a, B5 (the `VirtualFileNaming` descriptor column) | B8b |
| B12 | Consumer sweep + docs + final lift (reduced scope — Svelte only) | all in-scope blocks | — |

**Out of execution scope** (designs preserved; original block ids retained for traceability):

| # | Block | Status |
|---|---|---|
| B7 | React vertical (descoped, probe-gated — D-t) | DEFERRED (D-ab) — full design in "Deferred Verticals"; reopens only on reassessment with evidence from the landed seams after the Svelte proof |
| B9 | Astro vertical (D-v) | DEFERRED (D-ab) — same gate |
| B10/B11 | Angular facts + surface; Angular templates + TCB sidecar (D-w) | EXTRACTED (D-ac) — full designs + own go/no-go in `docs/arch/angular-adapter-program.md` |

**Critical path**: B2 → B3 → B4 → **{B5, B6}** → B8a → B8b/B8c → B12 (the whole program — with
React/Astro deferred and Angular extracted there are no parallel vertical lanes; B8b ∥ B8c is the
only post-substrate parallelism). B8a requires BOTH B5 (registry rows, synth seam,
`ScriptFactProvider` seam) and B6 (`CarrierCompiler`); B5 and B6 are mutually parallel-safe — D-n
removed the type dependency, and the registry-completeness guard is SPLIT so B5's
`framework_registry_complete` never references B6's `CarrierCompiler` (the compiler leg is B6's
`carrier_descriptors_have_compilers`, fix round 2). B2 → B3 is a SERIAL edge for scheduling: B3 is
semantically independent of B2 but its import-path sweep touches the same `verter_session` files
(`parse.rs`, `project_type_store.rs`, `host_manage*`, `host_resolve*`, `semantic_query*`) — the
orchestrator lands B3 after B2, never concurrently.
**The substrate proof is B5 itself** — the Vue framework-surface round-trip parity test + TS
decode exercises registry + executor + wire arm end-to-end (D-t). The Svelte vertical (B8a/b/c)
is the program's single non-Vue proof (D-ab).
Every block is independently landable, full-gate-green, and reviewable.

**Canonical verification gate (every block, referred to below as "the gate")**:

```bash
cargo nextest run --workspace                  # completeness (incl. verter_session integration suite)
cargo test -p verter_session --tests           # shared-process surface
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
pnpm install --frozen-lockfile
pnpm test                                      # where TS was touched
```

---

## 8. Blocks

### B1 — Wire completion

**Context.** The framework wire surface is landed but incomplete: `FrameworkSurfacePayload` exists
yet `TypeInfoGraphResponse` cannot return it. This block is the program's ONLY wire change (D-b as
revised by D-aa), done once, early, under the four typeinfo wire invariants. Per D-aa, NO new
`FrameworkTag` values land here — `FRAMEWORK_TAG_SVELTE = 2` already exists for the in-scope
vertical; Astro/Angular tags land with their own verticals.

**Changes.**
- `crates/verter_protocol/proto/verter/v1/typeinfo.proto`: `TypeInfoGraphResponse.kind` gains
  `FrameworkSurfacePayload framework_surface = 3`; **tag semantics pinned in doc comments
  (D-aa)** — the `FrameworkTag` enum gains a doc comment stating: *a `FrameworkTag` value's
  existence is NOT a support guarantee; support is asserted only by a registered adapter, and is
  surfaced per-request via `FrameworkSurfaceKindStatus`; new tag values land only together with
  their adapter's vertical* (the live `FRAMEWORK_TAG_REACT`/`FRAMEWORK_TAG_SOLID` values, which
  have no adapter, are exactly this situation already); the same sentence is mirrored in the
  `FrameworkAdapterDescriptor` rustdoc (B5) and the `/framework-adapters` skill; **per-kind
  status (D-s)**: `FrameworkSurfaceKindEntry` gains
  `FrameworkSurfaceKindStatus status = 3` with NEW message
  `FrameworkSurfaceKindStatus { FrameworkSurfaceKindSupport support = 1; GraphExactness
  exactness = 2; repeated GraphDiagnostic diagnostics = 3; }` and NEW closed enum
  `FrameworkSurfaceKindSupport { UNSPECIFIED = 0; SUPPORTED = 1; UNSUPPORTED = 2; PARTIAL = 3; }`
  (reuses the EXISTING `GraphExactness` — incl. `GRAPH_EXACTNESS_UNSUPPORTED` — and
  `GraphDiagnostic`; no parallel diagnostic vocabulary). Doc comments pin the semantics:
  SUPPORTED ⇒ `members` authoritative (empty = supported-empty); UNSUPPORTED ⇒ `members` empty +
  `exactness = UNSUPPORTED` + ≥1 diagnostic; PARTIAL ⇒ usable subset + explaining diagnostics;
  UNSPECIFIED invalid in server-produced v3 payloads; a v3 framework-surface response carries
  EXACTLY ONE entry per known `FrameworkSurfaceKind`.
- `crates/verter_protocol/src/typeinfo/graph.rs:254`: `TYPEINFO_GRAPH_SCHEMA_VERSION = 3`.
- `crates/verter_session/src/typeinfo/request_validation.rs:70,86`:
  `MIN_TYPEINFO_GRAPH_SCHEMA_VERSION` stays 2; `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS = &[2, 3]`.
- **Operation-minimum schema gate (D-s)**: `FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION = 3` +
  `validate_schema_version_for_operation(op, version)` — global membership first, then the
  per-operation minimum; applied in BOTH the envelope validation
  (`validate_type_info_graph_request`) and `validate_framework_surface_request`; failure (and
  envelope/payload version mismatch on framework-surface requests) is `MalformedPayload` with
  detail — NOT `UnknownSchemaVersion` (v2 stays globally supported), NOT a new error oneof arm
  (v2 clients could not decode one). The closed-contract semantics pinned EXPLICITLY: **schema 2
  is legacy-operations-only** — every pre-existing operation accepts `[2, 3]`; the
  framework-surface operation requires 3. `validate_schema_version_for_operation` matches
  EXHAUSTIVELY over the operation discriminant with NO wildcard arm — adding a future operation
  fails to compile until its op-minimum row is decided (the mechanism that makes "future
  operations cannot omit an op-minimum" structural, not conventional). Supported-set
  advertisement (round-3 citation correction — there is NO server handshake-response producer in
  the live tree): the `UnknownSchemaVersion` ERROR payload's `server_supported_versions` field,
  populated from `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` via `wire_error_unknown_schema_version`
  (`crates/verter_protocol/src/typeinfo/graph.rs:288-296`), reports `[2, 3]` — the constant is
  the single advertisement source.
- `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` rustdoc REWRITTEN in the same change: the current
  doc comment (`request_validation.rs:71-86`) states the single-version policy ("single-sourced
  from `TYPEINFO_GRAPH_SCHEMA_VERSION`: bumping the server schema adds the new version here and
  removes obsolete ones in the same commit") — keeping `[2, 3]` contradicts it. The new comment
  states the legacy-operations-only contract: the set holds every version some operation still
  accepts; schema 2 is legacy-operations-only; per-operation minimums live in
  `validate_schema_version_for_operation`; versions leave the set only when no operation accepts
  them. Code and policy text may not diverge.
- Regenerate Rust + TS bindings (`packages/proto/src/gen/verter/v1/typeinfo_pb.ts`) via the
  workspace `buf` + `oxfmt`; byte-pin test green.
- `crates/verter_audit`: additive audit coverage for `GRAPH_OPERATION_FRAMEWORK_SURFACES` through
  the existing `TypeInfoGraph` kind payload (new fields default-zero, additive only).

**Legacy Deletions.** None (additive-only by wire rule; no reserved-tag retirements needed).

**Tests (failing first).**
1. `crates/verter_session/tests/g_block/typeinfo_graph_taxonomy.rs` parity expectations extended
   for the new response arm + `FrameworkSurfaceKindStatus`/`FrameworkSurfaceKindSupport`
   (red until proto lands); NEGATIVE: the `FrameworkTag` variant set is asserted UNCHANGED
   (`NONE`/`VUE`/`SVELTE`/`REACT`/`SOLID`/`OPEN_CANONICAL` — D-aa; a stray tag addition fails the
   taxonomy guard).
2. `typeinfo_proto_ts_freshness.rs` byte-pin (red until regen).
3. `typeinfo_request_validation.rs`: schema 2 AND 3 accepted generically; 1 and 4 rejected with
   typed errors; **framework-surface op: schema 3 accepted, schema 2 rejected with
   `MalformedPayload` (DISCRIMINATING: asserts the error kind + that rejection happens before any
   adapter lookup/semantic dispatch); envelope/payload version mismatch rejected**; every LEGACY
   operation accepted at schema 2 (the legacy-ops-only contract asserted op-by-op, not sampled);
   the `UnknownSchemaVersion` error payload built for an out-of-set version carries
   `server_supported_versions == [2, 3]` (the advertisement surface — asserted through
   `wire_error_unknown_schema_version` fed by the supported-set constant, the single source);
   the rewritten `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` rustdoc states the
   legacy-operations-only contract (doc-vs-code divergence is the pre-change red); op-minimum
   exhaustiveness pinned (a `match` with no wildcard arm — compile-time — plus a unit test
   walking every operation discriminant through `validate_schema_version_for_operation`).
4. TS decode round-trip spec for the `framework_surface` response arm incl. per-kind status
   (`packages/proto`).
5. Negative: existing field numbers/tags unchanged (taxonomy guard re-run); **unsupported-vs-
   supported-empty distinguishability: a payload with `SUPPORTED` + empty members and one with
   `UNSUPPORTED` + empty members decode to distinct typed states** (the §9 contract's wire proof —
   adapter-level negatives land per vertical).
6. `request_kind_payload_parity.rs` green across the audit addition.

**Verification.** The gate (TS touched → `pnpm test` required).

**Dependencies.** None. Parallel-safe with B2/B3.

---

### B2 — `verter_language` + routing cutover

**Context.** Five duplicated kind enums + a mapping shim + a silent FFI `"vue"` default encode the
binary Vue/non-Vue worldview. One leaf authority replaces all of them in a single clean cutover
(D-a); Vue/TS routing behavior is byte-identical, and the FFI silent default becomes an explicit
classify-or-error (a flagged latent-bug fix).

**Changes.**
- NEW leaf crate `crates/verter_language` (deps: `verter_span` only): `FrameworkAdapterId`
  (interned), `LanguageId`, `FileLanguage` (D-a shape), `LanguageRegistry` with built-in rows for
  the TS/JS script family + `.vue` → `Framework { "vue" }` + `.svelte` → `Framework { "svelte" }`
  (D-ao): the `.svelte` row lands HERE, in the SAME block as the `"svelte"` FFI accepted string —
  a KNOWN-language row with NO carrier compiler, NO synth, NO provider, NO surface adapter behind
  it; the row is the STRUCTURAL source of the typed `UnsupportedLanguage` result (the
  pre-vertical-semantics bullet below), and today's unknown-extension behavior
  (`crates/verter_workspace/src/exact_resolution_tests.rs:655` pin) is superseded AT B2 by an
  explicit known-but-unsupported routing test. Per D-r the crate is PURE — static
  extension rows + gated-candidate descriptors via `classify_static(path)`; it never reads project
  config. The
  `.astro` row rides the deferred B9 design, and the `.html` gated-candidate row rides the
  extracted Angular program (D-ac) — neither lands in this program. `extension_priority` /
  `effective_target()` (`crates/verter_session/src/types.rs:1884,1918`) stay untouched — carriers
  remain priority 9.
- NEW `crates/verter_session/src/framework/{language_classifier.rs, project_capabilities.rs}`
  (D-r): `HostLanguageClassifier` composes `LanguageRegistry::classify_static(path)` with
  `ProjectCapabilitySnapshot` (trivially empty for this whole program — the extracted Angular
  program adds the first capability bit; the MECHANISM lands and is unit-tested here). TWO
  authority levels, pinned per crate (dependency direction is session → scheduler/workspace, so
  the host classifier is NOT literally callable from those crates): `verter_scheduler` /
  `verter_workspace` see ONLY `classify_static` directly; host-GATED classification reaches them
  exclusively through session-implemented trait objects (the `SourceLoader` impl /
  `WorkspaceAccess::classify_file`) — `HostLanguageClassifier` is the single classification
  authority every SESSION-level consumer calls, and the only producer behind those trait seams.
  Invalidation scoping (D-r, round-2): `ProjectCapabilitySnapshot.hash` (over DERIVED capability
  bits, never raw config bytes) keys the CLASSIFICATION cache only; the per-file
  `FileArtifactStore` key gains the explicit `file_language_id` column (the resolved
  `FileLanguage` row) so gated-row flips invalidate exactly the affected files' artifacts —
  NOTHING capability-shaped enters the global `parse_env_hash`. The SKILL.md key-composition
  table row for `FileArtifactStore` — and CLAUDE.md's hard-coded `FileArtifactStore` 4-tuple
  (see the key bullet below) — are updated in this block's docs sweep. This is the first
  resident of the `verter_session/src/framework/` substrate module (D-q fills it out in B5).
- Cutover consumers: `crates/verter_session/src/types.rs:17`,
  `crates/verter_scheduler/src/source_loader.rs:40` (the `SourceLoader::classify` return type),
  `crates/verter_scheduler/src/node.rs:173`, `crates/verter_workspace/src/types.rs:6` +
  `WorkspaceAccess::classify_file`, `crates/verter_session/src/host_executor.rs:151` parse
  dispatch, `crates/verter_session/src/parse.rs:111-142` (`imported_eval_source_type` branches on
  `FileLanguage`, not `ends_with(".vue")`), and the `get_public_api_with_mode` Vue gates
  (`host_resolve/virtual_file_pipeline.rs:1503/:1526` `file_kind != FileKind::VueSfc` — respelled
  over `FileLanguage` here mechanically; B5 replaces them with the D-ak registry dispatch).
- `FileArtifactStore` key gains the explicit `file_language_id` column (D-r):
  `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` — inert for
  static rows (extension-derived) and load-bearing once the first gated row lands (the extracted
  Angular program's `.html`); unit test pins
  that distinct `file_language_id` values occupy distinct artifact slots; the
  `/type-cache-architecture` SKILL key-composition table row AND CLAUDE.md's Cache Architecture
  section (it hard-codes the `FileArtifactStore` 4-tuple — "Keyed by `(canonical, content_hash,
  parse_env_hash, parser_version)`" — which gains the `file_language_id` column) are updated in
  this block's docs sweep.
- FFI: `crates/verter_ffi/src/convert/input.rs:163-169` → `ffi_file_language_to_host`; accepted
  strings extended add-only with `"svelte"` ONLY — PAIRED in this same block with the `.svelte`
  registry row per the pairing rule (D-ao): **an accepted string lands in the same change as its
  `LanguageRegistry` row; the row-without-carrier is the structural source of the typed
  `UnsupportedLanguage` intermediate state — an accepted string is never a dangling kind naming a
  `FileLanguage` no row produces** (deferred/extracted verticals add `"astro"` /
  `"angular_template"` with their own rows under the same rule); **absent kind →
  `LanguageRegistry::classify_static(canonical_path)` (the ONE method name — there is no separate
  `classify`); no path → typed error**. STATIC-ONLY semantics pinned: FFI-time classification
  cannot consult `ProjectCapabilitySnapshot`, so a gated-row extension (`.html`) can NEVER
  classify as `FrameworkTemplate` by inference at this boundary — gated rows REQUIRE an explicit
  kind string, else the typed error; nobody "fixes" the FFI by reaching for host state. NAPI/WASM
  JS boundaries get the one logged lenient-inference point.
- **Pre-vertical semantics for known-but-unimplemented languages (structural, tested at B2 —
  D-ao)**: a `FileLanguage::Framework { adapter_id }` produced by a landed registry row whose
  adapter has no registered carrier/parser yet (`.svelte`/`"svelte"` from B2 until B8a; any
  future vertical's id between its row and its vertical) is NOT silently
  empty and NEVER panics — `host_executor` returns a typed `UnsupportedLanguage { adapter_id }` error
  (surfaced as a typed FFI/LSP error, same family as the no-path classify error). The registry
  row WITHOUT a carrier is the structural source of this state — not prose, not a string check.
  LSP exposure is inert by construction: `carrier_extensions()` includes `.svelte` from B2 (the
  watcher glob widens), but no virtual-file wiring exists for it until B8c, so watched `.svelte`
  files produce no provider sync and direct requests surface only the typed error (asserted at
  B2). When the
  vertical lands, the same dispatch finds the registered carrier and the error path goes dead
  naturally (no flag, no dual branch).
- LSP: `crates/verter_lsp/src/server_utils.rs:41` `is_vue_file` → registry-backed
  `carrier_language_for(path)` (full rename, no deprecated alias);
  `crates/verter_lsp/src/capabilities.rs:163,173` watcher globs built from
  `LanguageRegistry::carrier_extensions()`.

**Legacy Deletions.** All four `FileKind` enums (paths above); the 1:1 shim
`WorkspaceSourceLoader::classify` mapping (`crates/verter_session/src/host_construction.rs:89-94`);
`ExportGraphFileKind` (`crates/verter_session/src/resolver_core/export_graph.rs:6`);
`ffi_file_kind_to_host` + the `unwrap_or("vue")` default; `is_vue_file`; the
`.svelte`-as-unknown-extension expectation in
`crates/verter_workspace/src/exact_resolution_tests.rs:655-668` (superseded at B2 by the
known-but-unsupported routing test — D-ao). All added to the
retired-symbol gate.

**Tests (failing first).**
1. Guard `single_language_classifier` (red against the live tree — five enums exist).
2. Guard `ffi_no_silent_vue_default` + behavior tests: absent kind + `.vue` path → vue; absent
   kind + `.ts` path → script (DISCRIMINATING: pre-change this silently produced VueSfc); absent
   kind + no path → typed error.
3. Characterization: Vue + TS routing byte-identical (existing compile/typeinfo/LSP suites are the
   pin; targeted before/after snapshot of host_executor dispatch for a `.vue` + `.ts` fixture pair).
4. `verter_language` unit tests: classify table, carrier extensions, unknown-extension fallthrough.
5. Known-but-unsupported semantics (D-ao, structural): `.svelte` classifies as
   `Framework { "svelte" }` through the B2 registry row (the routing test superseding the
   `exact_resolution_tests.rs:655` unknown-extension pin — DISCRIMINATING: red pre-B2, when
   `.svelte` was unknown); an FFI request with kind `"svelte"` (and a path-classified `.svelte`
   request) returns the typed `UnsupportedLanguage` error from the row-without-carrier dispatch —
   no panic, no silent empty (DISCRIMINATING: asserts the error kind, not just non-success);
   watcher-glob inertness — `.svelte` in `carrier_extensions()` produces no provider sync state
   (no virtual-file wiring exists until B8c).

**Verification.** The gate.

**Dependencies.** None. Parallel-safe with B1 (B3 is scheduled after B2 — see B3's dependency
note).

---

### B3 — Neutral script-analysis rehoming (FQ1)

**Context.** Framework-neutral OXC script inventory lives under the Vue-named path
`crates/verter_parser/src/utils/oxc/vue/script/` (`raw_surface.rs`, `resolve_type/`, the generic
half of `bindings.rs`). Under the owner's relaxation the lead's follow-up verdict is to physically
rehome it NOW — re-export aliases are not the landed end-state. The `resolve_type` name is also a
second-resolver lookalike; it is renamed `type_surface`.

**Changes.**
- NEW `crates/verter_parser/src/utils/oxc/script/`:
  - `raw_surface.rs` (+ `raw_surface_tests.rs`) — moved whole (`RawSourceSurface`, `SymbolSpace`,
    raw pre-lowering facts).
  - `type_surface/{mod.rs, external.rs, decl.rs, elements.rs, infer.rs}` — the former
    `vue/script/resolve_type/` moved + renamed, MINUS the Vue-staying symbols relocated per the
    table below; the sibling `#[path]`-included test files `resolve_type_tests.rs` and
    `resolve_type_typed_form_tests.rs` move with it (renamed `type_surface_tests.rs` /
    `type_surface_typed_form_tests.rs`); module docs state: local OXC-to-owned script surface
    capture, NO host-backed query resolution, NO cross-file semantic engine.
  - `bindings.rs` — the generic import/decl binding inventory split OUT of
    `vue/script/bindings.rs`.
- `crates/verter_parser/src/utils/oxc/vue/script/` keeps Vue semantics: `macros.rs`, `options.rs`,
  `setup.rs`, `shared.rs`, `types.rs`, `usage.rs`, and a thinned `bindings.rs` (Vue
  `<script setup>` binding/macro/ref classification, delegating to the generic inventory).
- **Symbol relocation table** (resolves the move/stay/delete ambiguity — `resolve_type/` is
  deleted as a PATH; every symbol in it has exactly one named destination):

  | Symbol (current `vue/script/resolve_type/`) | Destination |
  |---|---|
  | `build_type_context`, `TypeResolutionContext`, `InterfaceResolutionEntry`, `extract_companion_types` | `script/type_surface/mod.rs` |
  | `resolve_type_elements`, `resolve_type_elements_with_ctx{,_ref}`, `ResolvedElements`, `ResolvedProp`, `ResolvedMemberVisibility`, `BlockedType{,Surface}` | `script/type_surface/elements.rs` + `mod.rs` (`ResolvedProp` reads as the neutral object-property surface and keeps its name) |
  | `ResolvedEmit`, `ResolvedEmitSignature` | `script/type_surface/elements.rs` RENAMED in the same move — `ResolvedEmit` → `ResolvedNamedCallSignature` (a named call signature: event-name-bearing first-param literal + payload), `ResolvedEmitSignature` → `ResolvedCallPayloadForm`, and the `ResolvedElements.emits` field → `call_signatures`. "Emit" is Vue semantics, not a neutral script-surface concept — a neutral module exporting `ResolvedEmit` re-blurs the boundary B3 draws. Vue consumers (`macros.rs`, session normalizers) cut over to the new names in the same block; mechanical, full-suite-pinned, no alias survives |
  | `AnalyzedExternalTypeSource{,Stats}`, `AnalyzedExternalTypeSymbol{,Kind}`, `resolve_external_type{,_with_canonical}`, `ImportedTypeBinding`, `ExtractedTypeBindings` | `script/type_surface/external.rs` |
  | `infer_runtime_type`, `RuntimeType`, `format_runtime_types` | `script/type_surface/infer.rs` |
  | `ResolutionDiagnostic{,Kind}`, `DiagnosticLocation`, `ResolutionBudgetExceeded`, `take_last_resolution_budget_exceeded` | `script/type_surface/mod.rs` |
  | `cache_keys` module (`NamedTypeCache`, `ResolvedNamedTypeCacheKey`, `ResolvedTypeParamBindingCacheKey`) | STAYS Vue: relocated to `vue/script/named_type_keys.rs` (D-l — it is the `HostResolvedNamedTypeKey` inner identity) |
  | `find_macro_type_param` | STAYS Vue: relocated into `vue/script/macros.rs` (macro-call type-arg lookup is Vue semantics) |

  Any symbol discovered during implementation that is not in this table gets classified by the
  same rule (consumed by non-Vue script analysis → `script/type_surface`; meaningful only for Vue
  macros/SFC semantics → relocated under `vue/script/`) and the table in this doc is amended in
  the block's PR.
- Update ALL production imports in the same block. NOTE (corrected): the
  `verter_compiler/src/lib.rs:10-17` re-export is the whole-module `pub use verter_parser::utils;`
  — it is PATH-STABLE under this internal move and needs no edit; what changes are downstream
  `use` paths in `verter_session`/`verter_semantic` (`IndexedReady.external_type_analysis` +
  `RouteOwnedShallowEntry.external_type_analysis`
  (`crates/verter_session/src/project_type_store.rs:169, ~539`), `ShallowFileState.analysis`
  (`crates/verter_session/src/resolver_core/shallow_file_state.rs`), `host_manage*`,
  `host_resolve*`, `semantic_query*`, and every other `utils::oxc::vue::resolve_type` /
  `utils::oxc::vue::script` consumer of a neutral symbol).

**Legacy Deletions.** The old module paths
`crates/verter_parser/src/utils/oxc/vue/script/{raw_surface.rs, raw_surface_tests.rs,
resolve_type/, resolve_type_tests.rs, resolve_type_typed_form_tests.rs}` and the generic half of
`vue/script/bindings.rs`; the Vue-staying symbols (`cache_keys`, `find_macro_type_param`) are
RELOCATED to the named `vue/script/` files above, not left at the old path; NO Vue-path re-export
aliases survive the landed block (temporary aliases acceptable only inside the unmerged branch).

**Tests (failing first).**
1. Guard `neutral_script_analysis_not_under_vue_path` (red against the live tree).
2. Zero behavior change — the full workspace suite is the pin (this block is move + rename + the
   `bindings.rs` split; no semantic edits).
3. Split correctness: a focused test pinning that Vue `<script setup>` binding classification
   output is unchanged after delegation to the generic inventory.

**Verification.** The gate.

**Dependencies.** None semantically; SCHEDULED AFTER B2 (the import-path sweep touches the same
`verter_session` files B2 edits — `parse.rs`, `project_type_store.rs`, `host_manage*`,
`host_resolve*`, `semantic_query*`; "parallel-safe" would invite textual merge conflicts).
Parallel-safe with B1 only.

---

### B4 — `FrameworkParseArtifact` carrier cutover (FQ2)

**Context.** `IndexedReady.cached_parse: Option<Arc<ParsedSfc>>` hard-types the canonical
post-parse artifact to Vue — and it is NOT alone: the live tree carries SEVEN production
`cached_parse: Option<Arc<ParsedSfc>>` struct fields plus a family of `Option<&ParsedSfc>`
threading APIs, all inside `verter_session` (the D-af sweep; zero `ParsedSfc` references exist in
any other non-parser/compiler crate). The lead's follow-up picks the open wrapper with typed
common metadata + a private erased carrier — open-framework extensible without a `(id, dyn Any)`
bag, with downcast confined to the owning adapter. This block cuts over the ENTIRE class in one
pass: every neutral carrier/API goes `FrameworkParseArtifact`; every Vue-semantic leaf is
confined behind the blessed Vue downcast.

**Changes.**
- NEW `crates/verter_language/src/parse_artifact.rs` (D-g): `FrameworkParseArtifact { adapter_id,
  language_id, parser_version, common: FrameworkParseCommon, carrier: Arc<dyn CarrierParse> /*
  private */ }`; `FrameworkParseCommon { script_regions: Vec<ScriptRegion>, template_regions,
  style_regions, external_links, diagnostics: Vec<LanguageDiagnostic> }` (NO
  `public_component_seed` — deleted per D-g; D-n's synth ctx subsumes it);
  `ScriptRegion { span, source_type: ScriptSourceType, kind: ScriptRegionKind }` with closed
  `verter_language` enums `ScriptSourceType { Ts, Tsx, Js, Jsx, Dts }` /
  `ScriptRegionKind { Instance, Module, Frontmatter }` (D-g);
  `LanguageDiagnostic { span: verter_span::Span, severity, code: &'static str, message: String }`
  is a NEW neutral diagnostic struct defined in `verter_language` itself (the crate's
  `verter_span`-only dependency claim holds — no `verter_compiler` diagnostic type crosses the
  leaf boundary); `trait CarrierParse: Any + Send + Sync { #[doc(hidden)] fn
  __verter_as_any(&self) -> &dyn Any; }`.
- `crates/verter_session/src/parse.rs:122-142` `imported_eval_source_type` REWRITTEN to read
  `FrameworkParseCommon.script_regions[].source_type` uniformly for every carrier artifact (the
  `.vue` special-case branch calling `sfc_script_source_type(parsed, ...)` DELETES — the Vue
  producer populates `ScriptRegion.source_type` from the same `<script lang>` data at parse
  time); plain scripts keep `non_sfc_source_type`. This is part of B4's byte-identity
  characterization scope (the computed `SourceType` for every fixture is byte-identical
  before/after).
- **Carrier privacy (D-m — compiles across crates, unlike `pub(crate)`)**: `verter_language`
  exposes a PUBLIC `#[doc(hidden)] __carrier_downcast_ref::<T>(artifact, token:
  &CarrierAccessToken) -> Option<&T>` (+ `Arc` form) that checks `artifact.adapter_id ==
  token.adapter_id` first; `CarrierAccessToken { adapter_id, _private: () }` is minted ONLY
  inside `verter_language` (D-ba): a named token factory private to the crate runs during
  `LanguageRegistry` carrier-row construction and returns the minted token exactly ONCE, to the
  registry-construction caller, as the carrier row's registration proof — the `_private: ()`
  non-public field keeps out-of-crate struct literals uncompilable, and NO public arbitrary-id
  constructor (`new(adapter_id)`/`From`/`Default`) and NO public by-id token lookup exist
  (API-surface pin). Consumers (B4's `vue_parse()`, B5's descriptor construction) RECEIVE and
  reuse that token; none constructs one. NEW static guard
  `carrier_access_token_minted_only_in_verter_language` (this block): `CarrierAccessToken`
  struct-literal/constructor expressions appear ONLY in the owning `verter_language` minting
  files (`parse_artifact.rs` + the `LanguageRegistry` row-construction module) and explicit
  test fixtures.
  Blessed wrappers — the ONLY non-hidden entries: the session-side bare token-gated free helper
  `carrier_for::<T>(artifact, &CarrierAccessToken)` (+ `Arc` form) in NEW
  `crates/verter_session/src/framework/ctx.rs`, landing in THIS block (D-az) so `vue_parse()`
  routes through it and B4's own tree passes the guard's three-file allowlist — B5 EXTENDS the
  same module with `FrameworkAdapterCtx`, whose `carrier_for` method routes through this helper;
  and `CarrierCompilerCtx::carrier_for::<T>` (B6, compiler). The
  `carrier_downcast_confined_to_owning_adapter` static guard — not Rust visibility — is the
  enforcement authority.
- NEW `crates/verter_compiler/src/framework_common/vue_bridge.rs` (module skeleton +
  `VueParseCarrier { parsed: Arc<ParsedSfc> }` implementing `CarrierParse`, with typed
  `parsed()`/`parsed_arc()` accessors). Compiler-owned per D-m so BOTH the B6 `CarrierCompiler`
  impl (same file) and `verter_session` (existing dependency) can name it. **Flagged**: an
  additive `verter_compiler` file creation — zero edits to Vue parser/codegen modules (invariant
  2 untouched).
- **ALL SEVEN production parse-carrier struct fields cut over in this one pass (D-af)** — each
  `cached_parse: Option<Arc<ParsedSfc>>` field is REPLACED by
  `framework_parse: Option<Arc<FrameworkParseArtifact>>` (`None` = plain script):
  `IndexedReady` (`project_type_store.rs:158`), `RouteOwnedShallowEntry` (:532),
  `HostSourceData` (`host_executor.rs:30` — population at `host_executor.rs:186/:203`, view
  clones at `host_views.rs:134/:145`, upsert promotion at `host_upsert.rs:1248` all ride the
  field), `CompileInput` (`types.rs:1852`), `EffectiveFileState` (`types.rs:2110`),
  `ContentOverrideWithParse` (`types.rs:2124`), `ExternalTypeResolutionInputs`
  (`host_manage.rs:440`). The neutral THREADING APIs that pass the payload go neutral in the
  same change: `cached_route_owned_eval_state` (`host_resolve/route_owned_shallow.rs:151`),
  `build_template_analysis` (`host_manage/analysis_io.rs:49`),
  `build_snapshot_from_source_state` / `build_route_owned_snapshot_from_source_state` /
  `current_eval_state` / `current_eval_state_uncached` (`host_manage/eval_env.rs:284/319/372/390`),
  `build_eval_script_source` / `imported_eval_source_type_for` / `build_external_type_analysis` /
  `build_eval_env_and_external_type_analysis` (`host_manage/eval_program.rs:120/134/655/686`),
  and the `overlay_materialize.rs:354` local. Vue-SEMANTIC leaves keep `&ParsedSfc` and are
  CONFINED to the Vue bridge: the producers `parse_vue_snapshot` (`parse.rs:100`) /
  `build_vue_snapshot_from_parsed` (:143) wrap into `VueParseCarrier` at parse time; the Vue
  analysis builders (`parse.rs:704/728/759/777`) and the script extractors
  (`host_resolve/vue_script_extract.rs:82/108`) take the typed carrier obtained through the
  blessed accessor; `sfc_script_setup_type_params` / `apply_sfc_script_setup_type_params`
  (`host_manage/eval_program.rs:82/99`) are RELOCATED into `host_resolve/vue_script_extract.rs`
  so `host_manage/**` ends `ParsedSfc`-free. The provenance-counter family
  `route_owned_snapshot_cached_parse_hits` (`types.rs:2426` + snapshot/dump mirrors) is RENAMED
  `route_owned_snapshot_parse_artifact_hits` (semantics unchanged) so the retired `cached_parse`
  token gate is clean.
- The Vue artifact is populated at the existing parse point
  (`crates/verter_session/src/parse.rs:144` `build_vue_snapshot_from_parsed` vicinity) wrapping
  `ParsedSfc` in `VueParseCarrier`; a typed accessor `vue_parse(&FrameworkParseArtifact) ->
  Option<&Arc<ParsedSfc>>` in the live session Vue adapter module
  (`typeinfo/adapters/vue/`, downcasting via the Vue `CarrierAccessToken` — received from the
  `.vue` `LanguageRegistry` carrier-row registration proof at host construction, D-ba — THROUGH
  the `framework/ctx.rs` bare `carrier_for` helper this block lands, D-az — never the raw
  `__carrier_downcast_ref`) is the only call-site change existing readers see.

**Legacy Deletions — the FULL production `ParsedSfc` sweep disposition (D-af; exhaustive over
the live tree — `verter_scheduler`/`verter_workspace`/`verter_ffi`/`verter_napi`/`verter_wasm`/
`verter_lsp`/`verter_mcp` production code carries ZERO `ParsedSfc`/`cached_parse` references;
any site discovered during implementation that is not in this table is classified by the same
rule — neutral payload threading → REPLACE; Vue semantics → CONFINE — and the table is amended
in the block's PR).** All paths `crates/verter_session/src/`:

  | Site | Symbol | Disposition |
  |---|---|---|
  | `project_type_store.rs:158` | `IndexedReady.cached_parse` | REPLACE → `framework_parse` (field deleted) |
  | `project_type_store.rs:532` | `RouteOwnedShallowEntry.cached_parse` | REPLACE → `framework_parse` (field deleted) |
  | `host_executor.rs:30` | `HostSourceData.cached_parse` | REPLACE → `framework_parse` (field deleted; populations :186/:203, clones `host_views.rs:134/:145`, promotion `host_upsert.rs:1248` ride) |
  | `types.rs:1852` | `CompileInput.cached_parse` | REPLACE → `framework_parse` (field deleted) |
  | `types.rs:2110` | `EffectiveFileState.cached_parse` | REPLACE → `framework_parse` (field deleted) |
  | `types.rs:2124` | `ContentOverrideWithParse.cached_parse` | REPLACE → `framework_parse` (field deleted) |
  | `host_manage.rs:440` | `ExternalTypeResolutionInputs.cached_parse` | REPLACE → `framework_parse` (field deleted) |
  | `parse.rs:133` | `imported_eval_source_type(… Option<&ParsedSfc> …)` | REPLACE — rewritten to read `FrameworkParseCommon.script_regions[].source_type` (the dedicated bullet above) |
  | `host_resolve/route_owned_shallow.rs:151` | `cached_route_owned_eval_state` `Option<Arc<ParsedSfc>>` leg | REPLACE → neutral artifact |
  | `host_manage/analysis_io.rs:49` | `build_template_analysis` param | REPLACE → neutral artifact (Vue downcast at the Vue-template leaf via `vue_parse()`) |
  | `host_manage/eval_env.rs:284/319/372/390` | `build_snapshot_from_source_state` / `build_route_owned_snapshot_from_source_state` / `current_eval_state` / `current_eval_state_uncached` params | REPLACE → neutral artifact |
  | `host_manage/eval_program.rs:120/134/655/686` | `build_eval_script_source` / `imported_eval_source_type_for` / `build_external_type_analysis` / `build_eval_env_and_external_type_analysis` params | REPLACE → neutral artifact (script-content extraction downcasts at the Vue leaf) |
  | `host_manage/overlay_materialize.rs:354` | local `cached_parse: Option<Arc<ParsedSfc>>` | REPLACE → neutral artifact local |
  | `host_resolve/route_owned_shallow.rs:303-306` | route-owned cold-parse PRODUCER — direct `parse_sfc` call gated by `ends_with(".vue")`, local `cached_parse` (D-bb; the producing twin of the `overlay_materialize.rs:354` row) | REPLACE — the cold parse routes through the Vue carrier producer (wraps into `VueParseCarrier`/`FrameworkParseArtifact` at parse time) and the local goes `framework_parse: Option<Arc<FrameworkParseArtifact>>`; downstream `as_deref()` threading rides the neutral-API rows above |
  | `parse.rs:100/:144` | `parse_vue_snapshot` / `build_vue_snapshot_from_parsed` | CONFINE — the Vue producers; wrap `ParsedSfc` into `VueParseCarrier` at parse time |
  | `parse.rs:752/:769` (`parse_sfc` at `:755/:773`) | `build_script_analysis_from_source` / `build_style_analyses_from_source` — on-demand Vue re-parse producers (the lazy `get_analysis` path; callers `host_manage/analysis_io.rs:424/:435`) (D-bb) | CONFINE — Vue producers in the allowlisted Vue producer/builder file; they parse through the Vue carrier producer |
  | `host_manage/analysis_io.rs:93/:100` + `:237/:244` (+ `parse_vue_snapshot` call at `:185`) | the cold/merged-source re-parse halves of `build_template_analysis` (`:49`) / `compute_template_analysis_if_missing` (`:154`) — direct `parse_sfc` fallbacks and `Cow<ParsedSfc>` locals (D-bb) | CONFINE — RELOCATED behind the Vue bridge (the Vue template-analysis builder half joins the `parse.rs` producer side), so `host_manage/**` ends `parse_sfc`-free as well as `ParsedSfc`-free; the `host_manage` entrypoints keep neutral artifact threading |
  | `parse.rs:677` | `sfc_script_source_type` | CONFINE — RELOCATED into the Vue producer (populates `ScriptRegion.source_type` at parse time; the session call-site branch deletes per the `imported_eval_source_type` bullet) |
  | `parse.rs:704/728/759/777` | `build_script_analysis_from_parsed_with_diagnostic` / `build_export_signatures_from_parsed_with_diagnostic` / `build_script_analysis_from_parsed` / `build_style_analyses_from_parsed` | CONFINE — Vue analysis builders behind `vue_parse()` |
  | `host_resolve/vue_script_extract.rs:82/108` | `extract_vue_script_content` / `script_content_spans_from_parsed` | CONFINE — already the Vue extraction module; callers obtain the carrier via `vue_parse()` |
  | `host_manage/eval_program.rs:82/99` | `sfc_script_setup_type_params` / `apply_sfc_script_setup_type_params` | CONFINE — RELOCATED to `host_resolve/vue_script_extract.rs` (`host_manage/**` ends `ParsedSfc`-free) |
  | `types.rs:2426` (+ `:2549/:2695/:2805/:2894/:3032` mirrors) | `route_owned_snapshot_cached_parse_hits` counter family | RENAME → `route_owned_snapshot_parse_artifact_hits` (semantics unchanged) |
  | `host_executor.rs:36`, `parse.rs:128-129` | doc comments naming `cached_parse` | ride the rename |

The `cached_parse` TOKEN is added to the retired-symbol gate (workspace-wide production ban —
every field/counter above is renamed or deleted, so the gate is clean); the
`parsed_sfc_confined_to_vue_bridge` static guard (§6) pins the CONFINE rows' end-state, scanning
BOTH the `ParsedSfc` type token AND the `parse_sfc` producer-call token against the same
allowlist (D-bb). No dual field, no transition shim.

**Tests (failing first).**
1. Guard `carrier_downcast_confined_to_owning_adapter` (red once a deliberately misplaced fixture
   downcast is introduced in the guard's negative self-test; green on the landed tree); guard
   `carrier_access_token_minted_only_in_verter_language` (red against a deliberately misplaced
   fixture token construction in its negative self-test; the API-surface half asserts no public
   arbitrary-id constructor and no public by-id token lookup exist on `CarrierAccessToken` —
   D-ba).
2. Wrong-adapter downcast returns `None` (unit, `verter_language`).
3. Vue byte-identity characterization: IDE TSX output, component-meta payloads, typeinfo surfaces
   for a fixture SFC byte-equal before/after (plus the full existing suites as the broad pin);
   `imported_eval_source_type` parity matrix — `<script lang="ts"|"tsx"|none>` fixtures compute
   the same `SourceType` through `ScriptRegion.source_type` as the retired `ParsedSfc` branch;
   AND the codex-named cutover surfaces pinned byte-identically through the replaced carriers:
   `HostSourceData.source_type` (the authoritative parse-time `SourceType` — unchanged for every
   fixture), content overrides (`ContentOverrideWithParse`/`EffectiveFileState` — override
   upsert/effective-state round-trips produce identical analysis snapshots), eval-source
   building (`build_eval_script_source` output byte-equal for two-script SFCs), and route-owned
   shallow state (`cached_route_owned_eval_state` + the renamed
   `route_owned_snapshot_parse_artifact_hits` counter behavior — warm-hit counts identical).
4. Retired-symbol gate row for the `cached_parse` token (red pre-deletion — the live tree holds
   all seven carriers); guard `parsed_sfc_confined_to_vue_bridge` (red against a deliberately
   misplaced fixture reference in its negative self-test — one fixture per scanned token, a
   `ParsedSfc` type reference AND a bare `parse_sfc(…)` call, D-bb; green on the landed tree,
   with `host_manage/**` both `ParsedSfc`- and `parse_sfc`-free).
5. Perf: `canary_warm_hit_zero_alloc` + `perf_bounds/` green; before/after parse-path bench noted
   in the PR.

**Verification.** The gate.

**Dependencies.** B2 (`FrameworkAdapterId`/`LanguageId`), B3 (lands first to avoid conflicting
`IndexedReady` edits).

---

### B5 — Adapter registry + framework-surface executor + Vue re-housing

**Context.** The keystone AND the program's substrate proof (D-t): the U14-planned registry
abstraction, landed as the lead's two-phase trait (D-f) with the executor owning all resolution;
the Vue framework-surface round-trip parity test + TS decode proves registry + executor + wire
arm end-to-end. Vue is re-housed as a TRUE plan/normalize adapter: the Vue resolution delegates
RELOCATE out of `typeinfo/adapters/vue/` into the executor module (D-ax — module re-homing,
behavior byte-identical); the existing `impl VerterHost` methods are retained for current
consumers and call the relocated delegates.

**Changes.**
- The `crates/verter_session/src/framework/` substrate (D-q; joins B2's
  `language_classifier.rs`/`project_capabilities.rs`): `descriptor.rs` —
  `FrameworkAdapterDescriptor { id: FrameworkAdapterId, tag: FrameworkTag, supported_surfaces:
  &'static [FrameworkSurfaceKind], carrier_language: Option<LanguageId>, virtual_file_naming:
  Option<VirtualFileNaming> (D-x) }`; `registry.rs` — `FrameworkAdapterRegistry` built once at
  `VerterHost` construction (statically populated: `vue` now; per-vertical additions later;
  unknown wire ids → typed `malformed_payload` error with detail — NO new error-enum variant;
  descriptor construction RECEIVES the adapter's `CarrierAccessToken` from its `LanguageRegistry`
  carrier-row registration proof and never constructs one — `verter_language` is the sole minting
  authority, D-ba/D-m). Registration is TWO
  structural legs (D-ag): rows are `FrameworkRegistration { descriptor, surface:
  SurfaceRegistration }` with CLOSED `SurfaceRegistration { Adapter(Arc<dyn
  FrameworkSurfaceAdapter>), Deferred }` — carrier/synth/script-fact-provider legs ride the
  descriptor; an adapter id registered with `surface: Deferred` is served by the executor
  structurally (one entry per known kind, `UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` +
  surfaces-not-yet-registered diagnostic — the D-s vocabulary, no hand-rolled error path). At B5
  Vue registers with `surface: Adapter`; the arm exists for B8a's intermediate state.
- `crates/verter_session/src/framework/ctx.rs`: `FrameworkAdapterCtx` — the ADAPTER-VISIBLE ctx
  `plan_surfaces`/`normalize` receive — is a CLOSED op surface (D-am/D-as) exposing carrier
  metadata + validated facts ONLY; its public method set is EXACTLY this enumeration, pinned by
  an API-surface test:
  (1) `carrier_for::<T>(canonical)` — routes through the bare token-gated `carrier_for` free
  helper B4 lands in this same module (`framework/ctx.rs`, D-az); the
  ctx drives the artifact materialization INTERNALLY and hands back only the adapter's own typed
  carrier, never the host artifact wrapper; (2) `script_facts_for::<T>(canonical)` (validated
  read of D-o facts, drives stage 2 on demand). NO resolve method exists on it (D-as): the four
  resolve ops — `instantiate_public_type(canonical)` (the shared `Instantiate` key),
  `resolve_macro_payload(owner, selector)`, `project_path(base, path, mode)`,
  `shallow_surface(node)`, 1:1 with `PlannedDemand` (D-ai) — and `export_graph(…)` (the wire
  `SemanticTypeGraph` export for the payload) live on the EXECUTOR-PRIVATE resolve surface
  (`ExecutorResolveCtx`, module-private to the executor module
  `typeinfo/framework_surface/mod.rs`, never exported,
  never passed to adapter code); the executor consumes `PlannedDemand` and drives them itself —
  an adapter needing more semantic data adds a `PlannedDemand` variant, never a ctx op.
  `ensure_indexed_ready` is likewise NOT on the ctx —
  executor/session-private: the live `IndexedReady` carries `raw_source`/`eval_source`
  (project_type_store.rs:147-160), and handing it to adapters violates invariant 1's raw-source
  ban. No `ProjectSemanticDispatch`, no OXC, no raw/eval source, no content snapshots
  (`HostSourceData`/`EffectiveFileState`), no `StoreView`. Guard
  `framework_adapter_ctx_closed_surface`.
- `trait FrameworkSurfaceAdapter: Send + Sync { descriptor(); plan_surfaces(ctx, selector,
  requested) -> FrameworkSurfacePlan; normalize(ctx, resolved) -> FrameworkSurfaceDtoBundle }`
  (lead's shape verbatim; trait in `framework/`, impls in `typeinfo/adapters/<framework>/`).
  **`FrameworkSurfacePlan` is a CLOSED typed demand vocabulary (D-ai)**:
  `FrameworkSurfacePlan { items: Vec<PlannedResolve> }`,
  `PlannedResolve { kind: FrameworkSurfaceKind, demand: PlannedDemand }`, closed
  `PlannedDemand { PublicTypeInstance { canonical }, MacroPayload { owner, selector },
  PathProjection { base, path, mode }, ShallowSurface { node } }` — each variant maps 1:1 onto
  the corresponding EXECUTOR-PRIVATE resolve operation (`instantiate_public_type`,
  `resolve_macro_payload`, `project_path`, `shallow_surface` — D-as: these live on the
  executor's private resolve surface, NOT on the adapter ctx); NO variant carries source text,
  raw byte ranges standing in for source, OXC handles, closures, or raw `SemanticQueryKey`s; NO
  `Custom`/`Raw` escape arm exists; the executor matches EXHAUSTIVELY over `PlannedDemand` (no
  wildcard arm), so a new demand variant cannot compile without an explicit executor decision.
- **TWO-STAGE `ScriptFactProvider` seam (D-o)**: STAGE 1 — `ScriptFactProvider` trait
  (`syntax_gate()` + `capture(cx) -> Option<FrameworkScriptCandidates>`) +
  `FrameworkScriptCandidateSet` (the per-file PARSE-DOMAIN candidate collection — also the D-au
  synth ctx input) + the `FrameworkScriptFactSet`/`FrameworkScriptFacts` envelope in NEW
  `crates/verter_semantic/src/analysis/framework_facts.rs`; provider registration rows on the
  registry (`framework/script_facts.rs`); a NEUTRAL dispatcher wired inside the ONE shallow OXC
  pass (`build_script_analysis_with_scope_from_program`) from `ensure_indexed_ready` +
  route-owned shallow paths captures SYNTAX candidates only (live OXC + `lower_ts_type` allowed;
  import resolution + capability reads FORBIDDEN — the resolved `FileLanguage` row is the only
  host-derived input); candidates stored content-addressed on the per-file artifact
  (`framework_script_candidates` slot, key `(canonical, content_hash, parse_env_hash,
  parser_version, file_language_id, provider_id, provider_version)` — nothing capability- or
  provider-registry-shaped in the global `parse_env_hash`, D-r). STAGE 2 — session-owned
  resolved-symbol validation in `framework/script_facts.rs`, driven on demand by
  `FrameworkAdapterCtx::script_facts_for::<T>()`: resolves candidate import sources through the
  EXISTING import resolver / route facts, rejects userland look-alikes, consults the derived
  capability bits, emits validated `FrameworkScriptFacts` cached with sub-key `(provider_id,
  provider_version, consumed_capability_bits, project_identity, resolve_env_hash)` (NO
  `lib_env_hash` — D-ah) on the owner artifact identity; publishes only via
  `SignatureAdmission::Cacheable`, never re-touches an OXC arena. Dispatch cost (D-an): the
  `ActiveProviderIndex` (rebuilt once per registry construction / capability-snapshot change) is
  TWO exact-match maps over the closed `ScriptFactSyntaxGate` vocabulary —
  `by_carrier_language: Map<LanguageId, ProviderList>` and `by_import_specifier:
  Map<InternedSpecifier, ProviderList>`. The per-file active set is COMPUTED EMPTY before any
  provider invocation: carrier files look up `by_carrier_language[row]`; `Script` files union
  `by_import_specifier[spec]` over the file's already-collected raw import specifiers, behind an
  `is_empty()` fast path that skips even the per-specifier lookups when no specifier-gated
  provider is registered (the state of THIS whole program — Svelte's gate is
  `CarrierLanguage`). A per-`FileLanguage::Script` provider list does NOT exist — a
  specifier-gated provider (the extracted Angular program's `"@angular/core"` row is the stress
  case) is reachable only through its exact specifier key, so a TS file without that specifier
  never enters the provider dispatch loop, never evaluates a gate, never allocates a fact
  container. The empty-set path is the IDENTICAL pre-existing shallow code path — guard
  `script_fact_providers_zero_cost_on_miss`; stage-1/stage-2 boundary
  enforced by guard `script_fact_capture_is_syntax_only`. No production provider is registered
  in this block — Vue does NOT move onto the seam (its macro analysis is already inside the
  shallow pass; D-d keeps it Vue-only); the seam lands here so B8a (and every later vertical —
  deferred or extracted) registers providers without re-opening the pass.
- NEW `crates/verter_session/src/typeinfo/framework_surface/` — `mod.rs` is the
  `GRAPH_OPERATION_FRAMEWORK_SURFACES` executor (and the module-private `ExecutorResolveCtx`
  home); `vue_exec.rs` is the relocated Vue resolution-delegate module (the Vue re-housing
  bullet below). The executor:
  `VerterHost::resolve_framework_surface_with_audit(FrameworkSurfaceRequest)`:
  validate (`validate_framework_surface_request` + the D-s op-minimum gate) → intern id →
  registry lookup → resolve selector (default-export components via the synthesized `default`) →
  adapter plans (against the facts/carrier-only `FrameworkAdapterCtx`, D-as) → executor resolves
  each `PlannedDemand` through the shared engine via its private resolve surface → adapter
  normalizes (receiving the resolved results as data) → executor exports
  `FrameworkSurfacePayload` (live wire shape: `SemanticTypeGraph graph = 4` +
  `surfaces = 5`) through B1's `framework_surface` response arm, emitting EXACTLY ONE
  `FrameworkSurfaceKindEntry` per known kind with the D-s status semantics (unsupported kinds:
  `UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` + diagnostic — never a bare empty). Audited
  entry-point registered.
- **Vue re-housing (D-ax) — delegate relocation + a TRUE plan/normalize adapter; behavior
  byte-identical.** The live `typeinfo/adapters/vue/` directory IS the Vue resolution machinery
  today, which this block's own guards ban from adapter scope (`public_type.rs:56
  resolve_vue_public_type` — `ensure_indexed_ready` :90, `ProjectSemanticDispatch` :103,
  `SemanticQueryKey::Instantiate` :109; `surface.rs:198 resolve_vue_macro_surface` —
  `ctx.ensure_indexed_ready` :235/:615/:867, `IndexedReady.raw_source` :436-437). Per-file
  disposition:

  | Live file (`typeinfo/adapters/vue/`) | Disposition |
  |---|---|
  | `public_type.rs` (`resolve_vue_public_type`) | RELOCATE whole file → `typeinfo/framework_surface/vue_exec.rs` (the `impl VerterHost` entry stays the public API, body moved); file DELETED |
  | `surface.rs` (`resolve_vue_macro_surface` / `resolve_vue_macro_surface_with_ctx`, `vue_macro_dtos` / `vue_macro_dtos_with_ctx`, `navigate_param_to_object_surface`, `slice_canonical_span` + the JSDoc/return-type span slicing over `IndexedReady.raw_source`, `props_from_typeinfo_surface` / `emits_from_typeinfo_surface` / `slots_from_typeinfo_surface` + `model_prop_fields`, `normalize_jsdoc_body`, `VueMacroSurface`) | RELOCATE wholesale → `vue_exec.rs`; the three `*_from_typeinfo_surface` DTO producers are RESOLUTION legs, not normalizers — each takes `&dyn ResolverContext`, raises member values through `ctx.dispatch()`, and slices JSDoc from cache-owned raw source — so they live executor-side, driven by the private resolve ops; file DELETED |
  | `runtime_ctor.rs` (`runtime_constructors_from_type_expr` — a pure `TypeExpr` walk; zero ctx / dispatch / banned-class tokens) | STAYS adapter-side: a genuine thin typed-IR normalizer |
  | `store.rs` (`VueShallowMetadataStore` / `VueMacroDtoKey`; the `StoreView` reference) | RETIRED in this block (the D-p migration onto `FrameworkSurfaceDtoStore` — dispositioned above) |
  | `mod.rs` | REWRITTEN: module docs + `adapter` / `runtime_ctor` declarations + the B4 `vue_parse()` carrier accessor; NO re-export shim of any relocated name survives under `adapters::vue` |
  | NEW `adapter.rs` | `VueFrameworkAdapter` — the TRUE plan/normalize impl (below); the only resolution currency in the vue dir is declarative `PlannedDemand` data |

  `VueFrameworkAdapter` holds NO resolve entry point (D-as): `plan_surfaces(ctx, selector,
  requested)` emits, per requested surface, `PlannedDemand::PublicTypeInstance { canonical }`
  (the public component type, `PublicType` query level) and one `PlannedDemand::MacroPayload
  { owner, selector }` per requested macro surface kind (PROPS / EMITS / SLOTS / OPTIONS /
  EXPOSE / MODEL — the §9 Vue column); the EXECUTOR resolves each demand through its private
  ops — `instantiate_public_type` delegates to the relocated `resolve_vue_public_type`,
  `resolve_macro_payload` to the relocated `resolve_vue_macro_surface` → `vue_macro_dtos`
  pipeline — all byte-pinned; `normalize(ctx, resolved)` consumes the resolved results AS DATA:
  per-kind bundling of the resolved DTOs into the `FrameworkSurfaceDtoBundle`, D-s status
  assignment, runtime-constructor classification via `runtime_ctor.rs` over already-resolved
  `TypeExpr`s — no dispatch, no `ensure_indexed_ready`, no source access. Every production
  call-site of the relocated functions re-imports from `typeinfo::framework_surface::vue_exec`
  (`meta_resolve/projectors/define_shapes.rs:150/:238/:326/:371`,
  `meta_resolve/slot_binding_graph.rs:1151`, `resolver_core/component_meta/mod.rs:170`,
  `host_manage/component_meta_extract.rs:1246` (`normalize_jsdoc_body`), the
  `typeinfo/types.rs:269-275` doc links, plus the vue-adapter / meta / host-manage test
  modules) — an import-path-only sweep, byte-identical behavior, pinned by the existing Vue
  suites + `framework_surface_vue_roundtrip`.
- **Api-content producer seam (D-ak)**: NEW `framework/api_projector.rs` — trait
  `ComponentApiProjector { render_api(cx: ComponentApiProjectorCtx<'_>) -> Option<TscResponse> }`
  + the api-projector registration leg riding the registration row (like the carrier/synth legs).
  `VerterHost::get_public_api_with_mode`
  (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1484`) is REWIRED: the
  Vue-only hard gate (post-B2 spelled over `FileLanguage`) is REPLACED by registry dispatch to
  the canonical's adapter api-projector leg — no dual branch; the host method stays the SINGLE
  entry, so every production consumer (`sync_coordinator.rs:246`, `workspace_scanner.rs:790`,
  the four `background_drain.rs` sites, `sync_orchestration.rs` ×4, `documents/mod.rs:485`,
  `component_resolve.rs` ×3, `verter_tsc/checker.rs`, `verter_type_runtime` tsserver ipc ×4,
  NAPI `getPublicApi`, WASM) is untouched. NEW `framework/api_projectors/vue.rs` — the Vue leg
  delegates to the mechanically-extracted `render_vue_public_api_legacy` (the ENTIRE existing
  flow: `cached_tsc_extract`, `extract_tsc_state`, `generate_tsc_from_state`,
  `collect_external_types_from_loaded_files`, `sync_transitive_macro_type_dependencies` —
  unchanged, byte-pinned for BOTH `PublicApiMode`s incl. source maps + external macro types). An
  adapter with no api-projector leg (or `api_suffix: None`) returns `None` — exactly today's
  non-Vue behavior. The Svelte leg lands in B8a (D-ak).
- **One DTO store substrate (D-p)**: NEW `framework/surface_store.rs` — per-adapter typed
  sub-maps `FrameworkSurfaceStore<K, B>` (`K: Eq + Hash + Clone + Send + Sync + 'static`, the
  adapter's REAL structural key) erased behind the registry as `dyn ErasedFrameworkSurfaceStore`;
  generic key columns `{ surface_kind, query_level, canonical, owner_whole_hash }` + the typed
  adapter remainder — fully structural, NO lossy digest component, NO env dims (generation-scoped
  memo per D-p), NO adapter/normalizer version column; the store is CONTENT-ADDRESSED +
  fact-validated (D-aq — `owner_whole_hash` is a content hash deliberately IN the key; the
  content-free query-identity vocabulary does NOT apply to this store); value
  `{ dto_bundle: Arc<dyn FrameworkSurfaceDtoBundle>,
  read_set_signature, validated_at_generation }`, published only via
  `SignatureAdmission::Cacheable`. `VueShallowMetadataStore` MIGRATES onto it in this block —
  the Vue adapter remainder key is `{ macro_index, macro_kind }` (the former `VueMacroDtoKey`
  minus the lifted generic columns: `canonical`/`whole_hash`/`level_tag` become
  `canonical`/`owner_whole_hash`/`query_level`); warm reads validate the same fact + generation
  gates; cross-adapter reads merge the child surface's read-set into the parent (non-cacheable
  child ⇒ non-cacheable parent); byte-identity pinned by the existing Vue suites. Guard
  `framework_surface_store_key_structural`.
- Component-default synthesis seam (D-n, parse-domain per D-au): NEW `framework/synth.rs` —
  trait `ComponentDefaultSynth { synthesise(cx: ComponentDefaultSynthCtx<'_>) ->
  Option<ShallowValueSymbol> }` with `ComponentDefaultSynthCtx { canonical_id, language,
  script_analysis: &ScriptAnalysisSnapshot, script_candidates: &FrameworkScriptCandidateSet,
  framework_parse: Option<&FrameworkParseArtifact> }` — every field PARSE-DOMAIN (D-au: synth
  output lands in content-addressed `ShallowFileState` version-rooted by the owner
  `FileWholeHash`, so stage-2 validated facts are structurally banned from the ctx; stage-2
  currency stays query-side via `script_facts_for`; guard
  `component_default_synth_parse_domain_only`), dispatched at the exact shallow-analysis
  point that calls `vue_default_synth` today, selected via the registry descriptor; the Vue impl
  calls `synthesise_vue_default_value_symbol(&cx.script_analysis.macros)` unchanged. Mechanical
  rename `is_synthesised_vue_default` → `is_synthesised_component_default` (D-k).
- NAPI/WASM/MCP plumbing: decode/encode for the new response arm (the operation rides the existing
  `TypeInfoGraphRequest` envelope — binding work only).
- TS: NEW `packages/typeinfo/src/framework-surface.ts` — typed decode of `FrameworkSurfacePayload`
  → `FrameworkSurface { framework, kinds: Map<FrameworkSurfaceKind, { status, members }> }`
  (per-kind status surfaced, D-s) via the existing `type-graph-decode` path (D-i scope).
- Docs: new CRITICAL rule + `/framework-adapters` skill + `CRITICAL_RULE_GUARDS` rows, same change
  (§6).

**Legacy Deletions.** `is_synthesised_vue_default` (renamed); **`VueShallowMetadataStore` +
`VueMacroDtoKey`-as-store-key** (migrated onto `FrameworkSurfaceDtoStore`, D-p; both added to the
retired-symbol gate); **`typeinfo/adapters/vue/public_type.rs` + `typeinfo/adapters/vue/surface.rs`
DELETED** (contents relocated verbatim to `typeinfo/framework_surface/vue_exec.rs` per D-ax —
every production caller listed in the re-housing bullet is re-pointed in the same change; NO
re-export shim, alias, or dual call-path survives under `adapters::vue` for any relocated name).
The existing Vue host methods stay (current consumers): the retained host methods and the
executor's private resolve ops CONVERGE on the same relocated `vue_exec` delegate functions —
ONE semantic path with two entries into it, while the ADAPTER itself holds no resolve entry
point: `plan_surfaces` emits `PlannedDemand` data and `normalize` consumes resolved results
(D-as — the registry-fronted path is plan/normalize, never "the adapter calls the same
functions").

**Tests (failing first).**
1. `framework_surface_vue_roundtrip` e2e (red until executor lands): wire payload for a fixture SFC
   equals the known-good macro-surface DTOs from the existing host methods (parity assertion).
2. Guard `framework_surface_wire_executor_validates_first`: unknown adapter id / bad schema →
   typed error BEFORE dispatch.
3. Guard `framework_registry_complete` (per D-aa + D-ag: `REACT`/`SOLID` registered as explicit
   deferred/out-of-scope TAG rows; `SVELTE` carries a deferred TAG row here — the adapter id
   itself registers in B8a with `surface: Deferred`, then B8b flips the arm to the real adapter;
   the guard distinguishes carrier-leg registration from surface-adapter registration: every
   registered adapter id has a descriptor + EITHER a surface adapter OR the structural
   `SurfaceRegistration::Deferred` arm — the carrier-compiler completeness leg lives in B6's
   `carrier_descriptors_have_compilers`, keeping B5 free of any B6 type). A fixture registration
   with `surface: Deferred` round-trips through the executor as per-kind `UNSUPPORTED` + the D-s
   diagnostic (DISCRIMINATING: the structural arm, not an error string). The descriptor rustdoc
   carries the D-aa tag-semantics sentence verbatim.
4. Guard `framework_adapters_are_thin_no_second_resolver` (static).
5. Synth-seam characterization: Vue synth output byte-identical through the trait dispatch.
6. TS decode specs for `framework-surface.ts` (incl. per-kind status decode).
7. R6 meta-guard green with the new CRITICAL rule registered.
8. **DTO-store migration (D-p)**: Vue macro-surface DTOs byte-identical through
   `FrameworkSurfaceDtoStore`; warm-hit fact/generation validation behavior characterized
   (DISCRIMINATING: a cross-file carrier edit invalidates the entry exactly as the retired store
   did); key-structurality negatives — two distinct Vue adapter keys (`macro_index` 0 vs 1, same
   everything else) NEVER alias, and `PublicType` vs `FullMetadata` occupy distinct slots through
   the first-class `query_level` column; retired-symbol gate rows red pre-deletion.
9. **Per-kind status semantics (D-s)**: with an in-tree fixture adapter registering a partial
   `supported_surfaces` set, the executor emits exactly one entry per known kind; unsupported
   kinds carry `UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` + ≥1 diagnostic; supported-empty ≠
   unsupported (both decode states asserted distinct).
10. Guard `script_fact_providers_zero_cost_on_miss` (with a fixture provider whose syntax gate
    deliberately misses — asserts the no-provider path is byte-identical AND the gate-miss path
    does zero capture work / zero allocation).
11. Guards `framework_surface_store_key_structural` (incl. the whole-struct-destructure unit
    test), `script_fact_capture_is_syntax_only` (red against a deliberately misplaced
    fixture reference in the guard's negative self-test), and
    `component_default_synth_parse_domain_only` (static half — red against a fixture synth impl
    referencing `FrameworkScriptFactSet`; the whole-struct destructure pin on
    `ComponentDefaultSynthCtx`; the behavioral half lands with B8a's Svelte synth, D-au).
12. Two-stage seam unit coverage with an in-tree fixture provider: stage-1 candidates captured
    content-addressed (no capability input); stage-2 validation rejects a userland look-alike
    import source and refuses fact emission when the consumed capability bit is off
    (DISCRIMINATING both ways); stage-2 publishes only `SignatureAdmission::Cacheable` (an
    overflowed signature returns the value but never warms the slot).
13. `PlannedDemand` closed-vocabulary pin (D-ai/D-as): a whole-enum unit test (no `..`, no
    wildcard arm) walks every variant onto its EXECUTOR-PRIVATE resolve operation — adding a
    variant fails to compile until the executor dispatch and this test both decide it; a static
    assertion that no variant field is a string/`Arc<str>` source-text carrier, an OXC type, or
    a raw `SemanticQueryKey`.
14. Guard `framework_adapter_ctx_closed_surface` (D-am/D-as): the static-grep half red against a
    deliberately misplaced fixture reference in its negative self-test; the API-surface pin
    enumerates the ctx's TWO public methods against the blessed D-as list (DISCRIMINATING: a
    scratch `instantiate_public_type` — or `ensure_indexed_ready` — re-exposure on the adapter
    ctx fails both halves; the executor-private `ExecutorResolveCtx` is asserted unreachable
    from `typeinfo/adapters/**`).
15. **Api-producer seam (D-ak)**: Vue `get_public_api` / `get_public_api_with_mode(Testing)`
    byte snapshots for fixture SFCs (code + source map) identical through the registry dispatch
    — incl. an external-macro-type fixture (the `collect_external_types_from_loaded_files` +
    `sync_transitive_macro_type_dependencies` path rides unchanged); a registered adapter
    WITHOUT an api-projector leg returns `None` (today's non-Vue behavior, asserted); the
    `framework_registry_complete` api-leg clause red against a fixture descriptor declaring
    `api_suffix: Some` with no projector leg; static guard — non-Vue api projectors reference no
    semantic dispatch/OXC/query-time resolution (red against a deliberately misplaced fixture
    reference in its negative self-test).
16. **Vue delegate relocation (D-ax)**: behavior-frozen — the existing Vue suites (typeinfo
    vue-adapter tests, `meta_resolve` suites, component-meta payload pins) run green against the
    relocated `vue_exec` module with import-path-only diffs; DISCRIMINATING for the relocation
    itself — the `framework_adapter_ctx_closed_surface` static half is RED on the
    pre-relocation tree (the live `typeinfo/adapters/vue/{public_type.rs, surface.rs}` hit the
    banned-token scan) and green only once the delegates are relocated and the vue dir holds
    only `adapter.rs` / `runtime_ctor.rs` / `mod.rs` (+ the B4 `vue_parse()` accessor); a
    retired-path negative asserts no `adapters::vue` re-export of any relocated name resolves.

**Verification.** The gate (TS touched).

**Dependencies.** B1 (response arm), B2 (ids), B4 (carrier access). Parallel-safe with B6 (D-n —
no compiler-owned type is consumed).

---

### B6 — Compiler framework scaffold

**Context.** The carrier seam in `verter_compiler`: shared per-framework compiler substrate +
single host dispatch, with Vue wrapped by delegation (never edited). Establishes the test harness
(sourcemap e2e pattern, corpus generator extension, known-bug manifest) every vertical reuses.

**Changes.**
- `crates/verter_compiler/src/framework_common/` (D-h; the module skeleton + `vue_bridge.rs` with
  `VueParseCarrier` exist since B4): `trait CarrierCompiler: Send + Sync` — `adapter_id()`,
  `parse(source, opts) -> FrameworkParseArtifact`, `eval_source(source, artifact) -> Arc<str>`
  (the position-preserving blanking contract: script bytes at raw offsets, every other byte
  whitespace-blanked), `compile_ide(artifact, ct: &mut CodeTransform, opts) -> Result<IdeOutput,
  CompileUnsupported>`, `template_data(artifact) -> TemplateFacts`. There is NO
  `analyze_script_facts` method and NO `CarrierScriptFacts` type — script-fact extraction for
  EVERY framework (carrier or not) goes through the one `ScriptFactProvider` seam (D-o, landed in
  B5); the carrier compiler is parse/eval/IDE/template only. Unsupported `CompileTarget` bits →
  typed `CompileUnsupported` (invariant 4). Plus the compiler-side registry fn,
  `framework_common/ctx.rs` (`CarrierCompilerCtx::carrier_for::<T>` — the compiler-side blessed
  downcast wrapper, D-m), and shared TSX-projection helpers.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` EXTENDED: implements
  `CarrierCompiler` (around B4's `VueParseCarrier`) by delegating call-for-call to `parse_sfc` +
  the existing IDE pipeline inputs; the only Vue-adjacent code. Zero edits to Vue parser/codegen
  modules.
- `crates/verter_session/src/host_executor.rs`: the B2 `FileLanguage::Framework` branch now
  dispatches through the compiler registry for ALL carriers (Vue via the bridge) — **flagged**
  session-dispatch rehousing, byte-identity-pinned (D-h).
- Empty per-framework module stubs are NOT created (stub prevention); each vertical adds its own
  `src/<framework>/`.
- Test scaffolding: `scripts/gen-corpus-audit-tests.mjs` EXTENDED with a corpus-config table (root
  dir, framework id, request shape) — one idempotent script sweeps all corpora, no generator
  divergence; `crates/verter_session/tests/framework_known_bug_manifest.rs` + bijection guard (§5);
  reusable sourcemap-e2e assertion helpers cloned from `sourcemap_e2e_tests.rs` shape.

**Legacy Deletions.** None (the scaffold is additive; the Vue direct parse branch in
`host_executor` is replaced by the bridge dispatch in the same change — no dual branch retained).

**Tests (failing first).**
1. Vue-through-bridge byte-identity: full compile/IDE suites green + a targeted snapshot asserting
   `compile()` outputs for fixture SFCs are byte-equal pre/post dispatch rehousing.
2. Guard `framework_codegen_uses_code_transform` (static; seeded with a negative self-test).
3. `CarrierCompiler` contract tests against a minimal in-tree test carrier (eval_source length
   invariant: output length == input length; script bytes at raw offsets).
4. Guard `framework_known_bug_ledger_bijection` (manifest empty, bijection trivially green,
   non-empty enforcement self-tested).
5. Corpus generator idempotency re-run (`node scripts/gen-corpus-audit-tests.mjs` produces no
   diff).
6. Guard `carrier_descriptors_have_compilers` (the compiler-completeness leg split out of B5's
   `framework_registry_complete`): every carrier-bearing session descriptor has a registered
   `CarrierCompiler` — Vue-through-bridge satisfies it here; red if B6 lands the registry without
   the Vue bridge registration.

**Verification.** The gate.

**Dependencies.** B2, B4. Parallel-safe with B5.

---

### B8a — Svelte parser + shallow + synth

**Context.** The flagship carrier vertical. Svelte 5 runes-first with scoped legacy basics
(`export let` props + dispatcher emits only). The parse + shallow + synth half lands first so the
typeinfo adapter (B8b) and IDE projection (B8c) can proceed in parallel.

**Changes.**
- NEW `crates/verter_compiler/src/svelte/`: `parser/{tokenizer.rs, template_ast.rs, mod.rs}` (byte
  tokenizer → `ParsedSvelte`: instance + `<script module>` spans, template AST — elements,
  components (uppercase/dotted tags), attributes INCLUDING Svelte-5 event attributes
  (`onclick={...}` — plain case-sensitive attributes, the PRIMARY event path per D-u) and spread
  attrs, `{expr}`, `{#if}{#each}{#await}{#key}` (incl. `{#each}` WITHOUT an `as` item),
  `{#snippet}`/`{@render}`, `{@html}`, `{@attach …}` (5.29), declaration tags `{const …}`/`{let …}`
  (5.56) + legacy `{@const}` (one AST node family), `{@debug}`, directive attributes parsed for
  the FULL current syntax — `bind:` (incl. the two-expression function-binding form
  `bind:x={get, set}`), `class:`, `style:`, `use:`, `transition:`/`in:`/`out:` (+ modifiers),
  `animate:`, legacy `on:` — every directive parses without crash regardless of its
  SUPPORTED/OUT-OF-SCOPE status in the matrix below, supported special elements `<svelte:head>` /
  `<svelte:element this={...}>` / `<svelte:window|document|body>` / `<svelte:boundary>` /
  `<svelte:options>`, plus parse-without-crash coverage for the out-of-scope
  `<svelte:component>`/`<svelte:self>`/`<svelte:fragment>`, the component `<style>` span PLUS
  nested `<style>` elements inside template elements (opaque content, parse-without-crash) and
  CSS custom-property attributes (`--name={expr}` / `--name="…"` parsed as plain attributes —
  D-ap); `await` accepted as an
  ordinary expression position in template expressions (the await-expressions row below governs
  projection); reuses `verter_parser::tokenizer` byte primitives READ-ONLY). NO type lowering in
  this crate (D-o; the thin-adapters guard bans it).
- NEW `crates/verter_semantic/src/analysis/framework_facts/svelte.rs` (D-o, two-stage): the
  Svelte `ScriptFactProvider` STAGE-1 capture — syntax gate `FileLanguage::Framework
  { "svelte" }`; runes inventory off the live OXC AST in the one shallow pass into the
  `SvelteScriptCandidates` payload: `SveltePropsCandidate` (source span, `$props()` type lowered
  ONCE via `lower_ts_type`, generic-vs-annotation source form, `$bindable()` members +
  initializers, members annotated with a binding IMPORTED-AS-`Snippet`-CANDIDATE — recorded as
  `(local_binding, raw_import_source)` pairs, NOT validated here), module-vs-instance export
  inventory, legacy `export let` + `createEventDispatcher<E>()` type argument. STAGE-2
  (session): the snippet-typed classification becomes REAL only when the candidate's import
  source resolves through the existing resolver to the `svelte` package (`Snippet` from a
  userland module is rejected — structural, never name-string; the B8a negative test). Registered
  on the registry row.
- `CarrierCompiler` impl: `parse` → `FrameworkParseArtifact` (carrier `ParsedSvelte`),
  `eval_source` blanks everything but BOTH script contents at raw offsets.
- The `.svelte` `LanguageRegistry` row EXISTS since B2 (D-ao — landed there paired with the
  `"svelte"` FFI accepted string; routing + the typed `UnsupportedLanguage` known-but-unsupported
  state are already live). B8a registers the CARRIER behind it: the dispatch now finds the
  registered `CarrierCompiler` and B2's typed-error path for `"svelte"` goes dead naturally (no
  flag, no dual branch).
- `svelte_default_synth`: `ComponentDefaultSynth` impl (consuming the PARSE-DOMAIN
  `SvelteScriptCandidates` from the synth ctx, D-n/D-au — stage-2 currency never enters synth;
  the stage-2-validated snippet classification is a query-time surface concern, B8b SLOTS) —
  class-shaped `default` whose construct signature returns the
  `{ $props: Props }`-shaped instance + exported instance-script members; `import C from
  './C.svelte'` resolves through the one `Instantiate` identity, recursion-safe by query identity
  exactly like `.vue`-imports-`.vue`.
- **Svelte api-content producer (D-ak)**: NEW
  `crates/verter_session/src/framework/api_projectors/svelte.rs` — the Svelte
  `ComponentApiProjector` leg, a PURE declaration-shim renderer over the carrier's SHALLOW
  inventory + the synthesized default symbol/export inventory (no `Instantiate`, no semantic
  dispatch, no OXC at render time — static-guarded). Rendered declarations, in order: the D-at
  TYPE-ONLY IMPORT/RE-EXPORT PRELUDE — minimal `import type` / re-export lines derived from the
  carrier's shallow script/module import facts for every preserved type reference (named
  aliases, default imports, namespace imports, re-exports; unused imports dropped — the prelude
  is computed from the preserved-reference set, so the preserved refs resolve in the
  `.svelte.ts` module context); `type __VerterProps` (the `$props()` type /
  legacy export-let object — refs preserved verbatim, never eagerly inlined),
  `interface __VerterInstance { $props: __VerterProps; …instance-script exports }`,
  `declare const __VerterComponent: { new (...args: any[]): __VerterInstance }`,
  `export default __VerterComponent`, `<script module>` exports as top-level named declarations.
  `PublicApiMode::Testing` returns `None` (the testing surface is Vue-only, D-ak/D-al). No new
  content cache (pure cheap render over already-cached shallow inputs). This is the content
  behind B8c's `Foo.svelte.ts` api file and its tsserver import e2e.
- Registry registration `"svelte"` (tag `FRAMEWORK_TAG_SVELTE`): the CARRIER LEGS — carrier +
  synth + script-fact provider + api projector (D-ak) — register here with the STRUCTURAL `surface:
  SurfaceRegistration::Deferred` arm (D-ag); the executor serves every Svelte surface request as
  one entry per known kind with `UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` + the
  surfaces-not-yet-registered diagnostic until B8b replaces the arm with the real
  `SvelteFrameworkAdapter` (one field flip, no dual path, no prose-only "unsupported" state);
  `framework_registry_complete`'s api-leg clause holds from this registration (the descriptor's
  `api_suffix: Some(".ts")` is matched by the registered projector leg).
- **Svelte v1 scope matrix (current-docs audit — D-ad; extends and supersedes the D-u
  enumeration; binding for B8a/B8b/B8c).** Audited against the LIVE official documentation
  (`svelte.dev/docs/svelte/*`) on **2026-06-10**; audited Svelte version: **5.56.3** (latest
  stable; declaration tags landed in 5.56.0). Every current-docs surface appears below as
  SUPPORTED (with its projection design) or OUT-OF-SCOPE v1 (with the exact landed behavior).
  OUT-OF-SCOPE rows are deliberate scope, registered as out-of-scope rows — NEVER known-bug
  ledger rows. "Prelude" = the D-u unmapped ambient prelude in the projected `.svelte.tsx`;
  "void-checked" = the expression is projected (bytes verbatim, spans mapped) into a position
  that type-checks it without contributing a value, so hover/diagnostics inside the expression
  survive. A Svelte version refresh re-runs this audit before any scope claim changes.

  **Runes** (docs: `/$state`, `/$derived`, `/$effect`, `/$props`, `/$bindable`, `/$inspect`,
  `/$host`):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `$state`, `$state.raw`, `$state.snapshot` | SUPPORTED | prelude declarations; call sites verbatim (D-u) |
  | `$state.eager` (5.41) | SUPPORTED | prelude declaration (identity-typed `<T>(value: T) => T`); pure typing — no projection effect |
  | `$derived`, `$derived.by`; destructured deriveds; writable deriveds (5.25) | SUPPORTED | prelude; writable deriveds are plain TS `let` reassignment — no extra work |
  | `$effect`, `$effect.pre`, `$effect.tracking`, `$effect.root` | SUPPORTED | prelude — full namespace (D-u) |
  | `$effect.pending` | SUPPORTED (prelude only) | declared `(): number` so fixtures referencing it type-check; its await-expression TRIGGER is governed by the await row below |
  | `$props` destructuring / defaults / rest / renames; `$props<T>()`; annotation form | SUPPORTED | D-u verbatim-call-site model — TS types the destructuring |
  | `$props.id()` (5.20) | SUPPORTED | prelude `(): string` |
  | `$bindable` | SUPPORTED | prelude, `never` default (D-u) |
  | `$inspect`; `$inspect(...).with` | SUPPORTED | prelude returns the `{ with }` object form (D-u) |
  | `$inspect.trace` (5.14) | SUPPORTED | prelude declaration |
  | `$host` | SUPPORTED | prelude |
  | runes as class fields (`$state`/`$derived` in classes) | SUPPORTED | plain TS class fields once the prelude declares the runes — no extra projection |

  **Template syntax** (docs: `/if`, `/each`, `/key`, `/await`, `/snippet`, `/@render`, `/@html`,
  `/declaration-tags`, `/@const`, `/@debug`, `/@attach`, `/basic-markup`):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `{expr}` interpolation | SUPPORTED | JSX expression |
  | `{#if}/{:else if}/{:else}` | SUPPORTED | ternary projection |
  | `{#each}` — keyed, index, destructuring, `{:else}`, iterables | SUPPORTED | `.map()` projection |
  | `{#each}` without `as` item (`{#each { length: n }}`) | SUPPORTED | parser accepts the missing `as`; projects to an iteration with no item binding (source expression void-checked) |
  | `{#key expr}` | SUPPORTED | expression void-checked; children projected unwrapped |
  | `{#await expr}{:then v}{:catch e}` (+ shorthand forms) | SUPPORTED | ternary over a synthetic promise-state holder (D-u); `v: Awaited<typeof expr>` |
  | `{#snippet}` / `{@render}` — params w/ defaults + destructuring (no rest params), optional `{@render s?.()}`, implicit `children`, recursion, `<script module>` exported snippets (5.5), preceding-sibling visibility | SUPPORTED | typed local functions bridged through the `__verter_snippet` brand declarator (D-ae) — the binding carries `Snippet<[…]>` so `{@render}` calls AND snippet-as-prop assignability both check; structural `Snippet` typing via the stage-2-validated `svelte` import (B8a above); ORDERING (D-ap): all snippet declarators of a projected scope function are hoisted to the TOP of that scope function (source order, CodeTransform MOVE ops, bodies keep mapped spans) — Svelte scopes snippets to the whole lexical scope, so a preceding sibling's `{@render}` must not TDZ-error under the clean-type-check gate |
  | `{@html expr}` | SUPPORTED | expression projected into a string-accepting checkable position; no inner-markup typing |
  | Declaration tags `{const x = …}` / `{let x = …}` (5.56) — identifier/object/array targets, sibling-scope visibility | SUPPORTED | declaration bytes verbatim as `const`/`let` in the projected scope function; sibling visibility realized by wrapping the following sibling run in a scope function (CodeTransform inserts only — no original-byte rewrites); TS types all target forms |
  | `{@const}` (documented LEGACY since 5.56) | SUPPORTED | same AST node family + same projection as declaration tags |
  | `{@debug var1, var2}` | SUPPORTED | projected as a void expression referencing the listed bindings (hover + type-check preserved) |
  | **`{@attach expr}`** (5.29, stable; owner-named) | SUPPORTED | on ELEMENTS: attachment expression projected (bytes verbatim) as the argument of an unmapped prelude checker `__verter_attach<E extends EventTarget>(a: import("svelte/attachments").Attachment<E>)` instantiated at the host element's intrinsic element type, spread-merged so the JSX stays valid; on COMPONENTS: a symbol-keyed prop riding spreads — same checker with the element contract erased to `Element`; `createAttachmentKey`/`fromAction` are ordinary `svelte/attachments` imports (no projection work) |

  **Event handling + attributes** (docs: `/basic-markup`, `/class`):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | Event attributes `onclick={…}` — case-sensitive, shorthand `{onclick}`, spreadable | SUPPORTED | PRIMARY event path (D-u): projected VERBATIM lowercase against the D-ae Svelte JSX namespace (`SvelteHTMLElements` is lowercase; NO `onClick` rename) |
  | Spread attributes + ordering/merge semantics | SUPPORTED | JSX spreads (order preserved) |
  | `class` attribute object/array clsx forms (5.16) | SUPPORTED | class value projected through a prelude checker typed `ClassValue` (`svelte/elements`); plain string `class` unchanged |
  | `class:` directive (legacy-discouraged, NOT deprecated) | SUPPORTED (legacy coverage) | condition expression void-checked; the class name itself is not a typed surface |
  | `style:` directive (+ `\|important` modifier) | OUT-OF-SCOPE v1 | parse-without-crash; directive recorded + stripped from projection; typed-unsupported diagnostic on the directive span; value expression void-checked |

  **Bindings** (docs: `/bind`):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `bind:value`, `bind:checked` | SUPPORTED | checkable value/on-change pair (D-u) |
  | Component `bind:prop` for `$bindable` props | SUPPORTED | D-u |
  | `bind:group`, `bind:files`, `bind:this`, contenteditable/details/media/dimension/readonly bindings; `defaultValue`/`defaultChecked` interplay (5.6) | OUT-OF-SCOPE v1 | parse-without-crash; bound expression void-checked; typed-unsupported diagnostic naming the binding |
  | Function bindings `bind:x={get, set}` (5.9; readonly `{null, set}`) | OUT-OF-SCOPE v1 | parser accepts the two-expression form; both expressions void-checked; typed-unsupported diagnostic |

  **Directives** (docs: `/use`, `/transition`, `/animate`):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `use:action` (+ parameter) | SUPPORTED | basic action parameter checking (D-u). Docs recommend attachments for ≥5.29 but `use:` is NOT deprecated — both are supported surfaces |
  | `transition:` / `in:` / `out:` (+ params + `\|local`/`\|global` modifiers) | OUT-OF-SCOPE v1 | parse-without-crash; stripped from projection; typed-unsupported diagnostic; params void-checked |
  | `animate:` (+ params) | OUT-OF-SCOPE v1 | same behavior as transitions |

  **Styling** (docs: `/scoped-styles`, `/global-styles`, `/custom-properties`,
  `/nested-style-elements` — D-ap; the docs-ToC Styling section, explicitly dispositioned):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | Scoped styles (hash class, specificity, scoped `@keyframes`) | OUT-OF-SCOPE (CSS domain) | NO type-facing surface; the component `<style>` block is an opaque recorded span (B8a parser), stripped from projection; no diagnostic (valid Svelte) |
  | Global styles — `:global(...)`, `:global {}` blocks, `-global-` keyframes | OUT-OF-SCOPE (CSS domain) | same behavior — opaque span inside the `<style>` block, never interpreted |
  | CSS custom properties — `--name={expr}` / `--name="value"` attributes on components and elements | SUPPORTED (pass-through) | parsed as plain attributes; STRIPPED from the projected JSX attribute position (a `--`-prefixed name is not a valid JSX attribute identifier — verbatim projection would break TSX validity); `{expr}` values projected void-checked so hover/diagnostics inside the value survive; no diagnostic |
  | Nested `<style>` elements (inside template elements) | OUT-OF-SCOPE (CSS domain) | parse-without-crash; opaque content; stripped from projection |

  **Special elements** (docs: `/svelte-window`, `/svelte-document`, `/svelte-body`,
  `/svelte-head`, `/svelte-element`, `/svelte-boundary`, `/svelte-options`):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `<svelte:window>` / `<svelte:document>` / `<svelte:body>` / `<svelte:head>` | SUPPORTED | event-binding projection (D-u) |
  | `<svelte:element this={expr}>` | SUPPORTED | conservative intrinsic typing (D-u) |
  | `<svelte:boundary>` (5.3) — `failed`/`pending` snippets, `onerror` | SUPPORTED | children + snippet props through the standard snippet machinery: `failed: Snippet<[unknown, () => void]>`, `pending: Snippet<[]>`, `onerror: (error: unknown, reset: () => void) => void` |
  | `<svelte:options>` — `runes`, `namespace`, `customElement`, `css` (legacy `immutable`/`accessors` deprecated) | SUPPORTED (parse + flags) | parsed + recorded; the `runes` flag drives runes-vs-legacy classification; `namespace`/`customElement`/`css` recorded with NO v1 typing effect — a non-html `namespace` (svg/mathml) additionally emits a typed-unsupported diagnostic (intrinsic-table switching is out-of-scope v1) |
  | `<svelte:component>` / `<svelte:self>` (deprecated in runes mode) | OUT-OF-SCOPE v1 | parse-without-crash + typed-unsupported diagnostic (D-u row re-affirmed) |
  | `<svelte:fragment>` (legacy slot construct) | OUT-OF-SCOPE v1 | parse-without-crash + typed-unsupported diagnostic |

  **Await expressions** (docs: `/await-expressions` — EXPERIMENTAL, 5.36+, opt-in via
  `compilerOptions.experimental.async`; the docs state the flag "will be removed in Svelte 6" and
  that handling details "are subject to breaking changes outside of a semver major release";
  owner-named):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `await` at instance-script top level / inside `$derived(...)` / in markup expressions | OUT-OF-SCOPE v1 | parse-without-crash in ALL three positions (markup awaits are ordinary template-AST expressions; script-level await parses as module-level await under OXC); the projector emits a typed-unsupported diagnostic (slug `svelte-await-experimental`) per await-bearing position and projects the awaited expression void-checked so inner type errors + hover survive. Rationale: explicitly experimental + opt-in + breaking-change-reserved — a proof vertical must not chase an unstable contract. Revisit gate: surface stabilisation (flag removal / Svelte 6) — a named follow-up, NOT a known-bug row. Companions type-check TODAY via rows above: `<svelte:boundary pending>`, `$effect.pending`, and `settled()` (an ordinary `svelte` import) |

  **Modules + runtime surfaces** (docs: `/svelte-js-files`, `/stores`, `/context`,
  `/lifecycle-hooks`, `/imperative-component-api`, `/hydratable` (D-ap), legacy pages):

  | Surface | Status | Design / exact behavior |
  |---|---|---|
  | `.svelte.ts` / `.svelte.js` rune modules | OUT-OF-SCOPE v1 | classify as plain `FileLanguage::Script` and serve the REAL file (D-u re-affirmed); rune identifiers in them resolve only through the user's own ambient setup; no carrier participates, so no Verter diagnostic is emitted |
  | Stores + `$x` auto-subscription | OUT-OF-SCOPE v1 | `$`-prefixed identifiers parse as plain identifiers and project verbatim; an unresolved `$x` surfaces as an ordinary type error; no auto-subscription typing (D-u re-affirmed) |
  | Lifecycle / context / imperative APIs (`onMount`, `getContext`, `mount`, `hydrate`, `settled`, …) | SUPPORTED (transparent) | ordinary `svelte` package imports typed by the package — zero adapter work |
  | Hydratable data — `hydratable(key, fn)` (docs `/hydratable`, Runtime; D-ap) | SUPPORTED (transparent) | an ordinary `svelte` import typed by the package; runtime JS API only — no template/component syntax surface, zero adapter work |
  | `$$props` / `$$restProps` / `$$slots` | OUT-OF-SCOPE v1 | parse fine (plain identifiers); typed-unsupported diagnostic in legacy-mode fixtures (D-u re-affirmed) |
  | LEGACY coverage set: `export let` props; `createEventDispatcher<E>()` emits; DOM `on:` projection; legacy `<slot>` inventory | SUPPORTED (legacy coverage — separate fixtures, never the primary path) | D-u verbatim, casing per D-ae: unmodified `on:click` carries the namespaced attribute verbatim (typed by `SvelteHTMLElements`' quoted keys); modifier forms lower through a typed helper against the base quoted key; NEVER `onClick` |
  | Legacy component `on:` payload checking | OUT-OF-SCOPE v1 | D-u re-affirmed: parse-without-crash + typed-unsupported diagnostic |
  | SvelteKit-layer surfaces (remote functions, `load` typing, `$app/*`) | OUT-OF-SCOPE (program) | a different product layer, not component-template surface; no parser/projector contact |

**Legacy Deletions.** B2's known-but-unsupported behavior tests for `"svelte"` (the
`UnsupportedLanguage`-on-dispatch and watcher-inertness assertions of B2 test 5 — superseded by
this block's positive parse/routing tests; the typed `UnsupportedLanguage` path itself STAYS in
the substrate as the structural pre-vertical state for every future row, D-ao. The
`.svelte`-as-unknown-extension expectation was already deleted in B2.)

**Tests (failing first).**
1. Parser round-trips + template-AST snapshots covering the FULL v1 scope matrix (D-ad) — every
   SUPPORTED row's constructs (event attributes, snippets, declaration tags, `{@attach}`,
   `{@html}`, `<svelte:boundary>`, clsx class forms, …), parse-without-crash fixtures for every
   OUT-OF-SCOPE row (function bindings, `style:`, transitions, await-expression positions,
   `<svelte:fragment>`, …), AND the legacy set in separate fixtures (vendored,
   `crates/verter_compiler/tests/framework_fixtures/svelte/`, version-pinned to the audited
   Svelte 5.56.x per D-ad).
2. eval_source invariants (length-preserving; script bytes at raw offsets; module script
   included).
3. Shallow inventory: `ShallowFileState` for a `.svelte` fixture contains the synthesized
   `default` (`is_synthesised_component_default = true`) + exported members.
4. Cross-file: TS file importing `.svelte` resolves the public type through `Instantiate`;
   circular `.svelte ↔ .svelte` import terminates (query-identity recursion test).
5. Userland look-alike NEGATIVE: a fixture importing `Snippet` from a userland module (NOT the
   `svelte` package — e.g. `./fake-svelte`) does NOT classify the member as snippet-typed
   (DISCRIMINATING: the resolved-symbol validation stage rejects it; a raw-name match would
   pass it). The mirror of the extracted Angular program's userland-`input()` negative.
6. Known-bug rows for any pre-existing semantic gaps surfaced (ledger + bijection guard); every
   D-ad OUT-OF-SCOPE row registered as out-of-scope (never a ledger row), each with its
   parse-without-crash + typed-unsupported-diagnostic behavior asserted (DISCRIMINATING: the
   diagnostic kind, not mere non-success).
7. Deferred-surface intermediate state (D-ag): a framework-surface request for a `.svelte`
   component at THIS block's tree returns one entry per known kind with `UNSUPPORTED` +
   `GRAPH_EXACTNESS_UNSUPPORTED` + the surfaces-not-yet-registered diagnostic — served from the
   structural `SurfaceRegistration::Deferred` arm (DISCRIMINATING: asserts the per-kind status
   payload, not an error); `framework_registry_complete` green with the Svelte id registered on
   carrier legs only.
8. **Svelte api-content (D-ak, failing-first)**: `get_public_api` for a `.svelte` fixture
   returns the declaration shim (red pre-block: `None`) — snapshots assert the
   `export default __VerterComponent` class shape with `$props: __VerterProps`, instance-script
   exports as INSTANCE members (and absent as module named exports — negative), `<script
   module>` exports as top-level named declarations, prop type REFS preserved un-inlined
   (negative: an imported props alias stays a reference in the shim text) WITH the D-at
   type-only import/re-export prelude — fixtures cover a named alias
   (`import type { Props as P }`), a default import, a namespace import, and a re-export, each
   asserting the prelude line is present and the referenced type is NOT inlined (negative);
   `get_public_api_with_mode(Testing)` returns `None` for `.svelte` (DISCRIMINATING: asserts
   `None`, distinct from the Public mode's `Some`); the non-Vue-projector static guard green.
9. **Synth parse-domain invariance (D-au — the `component_default_synth_parse_domain_only`
   behavioral half)**: the synthesized `default` symbol for a candidate-bearing `.svelte`
   fixture is structurally IDENTICAL whether the workspace carries the real `svelte` package or
   the fake one (and across capability state) — synth output is a pure function of parse-domain
   inputs (DISCRIMINATING: a synth impl reading stage-2 facts would diverge); the
   stage-2-dependent snippet classification surfaces ONLY through query-time consumers (test 5
   here; B8b SLOTS), never through shallow state.

**Verification.** The gate.

**Dependencies.** B5 (registry registration, synth seam, `ScriptFactProvider` seam) + B6
(`CarrierCompiler`) (+B2 routing mechanism).

---

### B8b — Svelte typeinfo adapter

**Context.** The surface seam for Svelte on the generic DTO store, mapping runes semantics onto
the six wire kinds.

**Changes.**
- NEW `crates/verter_session/src/typeinfo/adapters/svelte/{mod.rs, adapter.rs}`:
  `SvelteFrameworkAdapter` on `FrameworkSurfaceDtoStore` — the post-D-ax adapter shape
  (plan/normalize only; no resolution leg lives under `typeinfo/adapters/`, matching the Vue
  layout B5 lands). **Store key remainder (D-bc, completes D-y)**: the Svelte adapter remainder
  on `FrameworkSurfaceDtoStore` is `{ source: SvelteSurfaceSource }` with the CLOSED
  source-family discriminant `SvelteSurfaceSource { RunesProps, LegacyExportLet, Bindable,
  SnippetProps, LegacySlotInventory, LegacyDispatcher, InstanceExports }` — the typed
  `Eq + Hash` structural remainder (D-y) parallel to Vue's `{ macro_index, macro_kind }`: a
  Svelte component has at most ONE declaration site per source family, so the family
  discriminant alone is the minimal remainder (no index column); kinds composed from two
  families (SLOTS = snippet-typed props + legacy `<slot>` inventory) occupy two source rows
  merged at normalise time, keeping each cached bundle single-source; pinned by extending the
  `framework_surface_store_key_structural` whole-struct destructure test with the Svelte row.
  Mapping (§9): PROPS = `$props()` type
  members (incl. snippet-typed) or legacy `export let`; MODEL = `$bindable()` props; SLOTS =
  snippet-typed props + legacy `<slot>` inventory; EMITS = legacy dispatcher event map ONLY
  (runes-mode callback props stay PROPS — Svelte 5 semantics); EXPOSE = exported instance-script
  members. Each kind = ONE planned demand (`PlannedDemand`, resolved by the executor through
  its private resolve surface — D-as) + thin normalise.
- Registry registration updated (D-ag): the Svelte `SurfaceRegistration::Deferred` arm is
  REPLACED by `SurfaceRegistration::Adapter(SvelteFrameworkAdapter)` — one field flip; the B8a
  deferred-state test is superseded by the e2e round-trip below in the same change.

**Legacy Deletions.** The B8a deferred-surface intermediate-state test (superseded by the
registered-adapter e2e — the `Deferred` arm itself STAYS in the substrate: it is the structural
intermediate state for every future vertical).

**Tests (failing first).**
1. `typeinfo_tests/svelte_adapter.rs` (+ `svelte_runes.rs`): runes + legacy fixtures, imported
   components, recursive imports, executor e2e (`framework_surface` round-trip), cache warm hits
   (DTO-store fact validation).
2. Negative: runes-mode `onClose` callback in PROPS, absent from EMITS.
3. Known-bug rows for surfaced pre-existing typeinfo gaps.
4. Corpus rows under `framework_corpus/svelte/` via the extended generator.

**Verification.** The gate.

**Dependencies.** B5 + B8a. Parallel-safe with B8c.

---

### B8c — Svelte IDE TSX + LSP

**Context.** The IDE projection: one valid TSX file with position-preserving spans + sourcemap —
the LSP parity contract.

**Changes.**
- NEW `crates/verter_compiler/src/svelte/ide/`: `ParsedSvelte` → TSX via `CodeTransform`:
  **the Svelte JSX TYPE ENVIRONMENT (D-ae)** — the UNMAPPED prelude (inserted at output offset 0
  via `prepend_left`) OPENS with the per-file pragma `/** @jsxImportSource @verter/svelte-jsx */`
  (overrides the provider's project-level `jsxImportSource: "vue"` for this file only, incl.
  under `jsx: "preserve"`); the Verter-owned shim `@verter/svelte-jsx/jsx-runtime.d.ts` (+
  `jsx-dev-runtime` re-export) is path-mapped into the inferred project through the provider's
  existing `paths` configuration and declares the `JSX` namespace —
  `Element = ReturnType<Snippet>`, `ElementClass {}`, `ElementAttributesProperty { $props: {} }`
  (component tags check props against the class-shaped synth's `$props`; an imported `.vue`
  component checks through the same contract), `IntrinsicElements extends SvelteHTMLElements`
  (`svelte/elements` — Svelte-true intrinsics; element typing below is THIS table, never Vue's);
  the shim never stubs the `svelte` package itself — a workspace without `svelte` fails CLOSED
  (module-not-found diagnostics + the typed `svelte-package-missing` diagnostic on the source
  file, D-ae(d));
  **runes typing via the per-file ambient prelude (D-u, completed per D-ad)** — the same prelude
  declares the COMPLETE audited rune
  surface: `$props`(+`.id`)/`$bindable`/`$state`(+`.raw`/`.snapshot`/`.eager`)/`$derived`(+`.by`)/
  `$effect`(+`.pre`/`.tracking`/`.root`/`.pending`)/`$inspect`(+`(...).with`/`.trace`)/`$host`
  (+ `import type { Snippet } from "svelte"`), plus the THREE prelude checkers/declarators —
  `__verter_attach<E extends EventTarget>(a: import("svelte/attachments").Attachment<E>)` (the
  `{@attach}` projection target), the `ClassValue`-typed class checker (`svelte/elements`), and
  the snippet-brand declarator `__verter_snippet<Params extends unknown[]>(render: (...args:
  Params) => unknown): Snippet<Params>` (D-ae(c));
  rune CALL SITES are preserved verbatim (`let { a, b = 1, ...rest }: Props = $props()` keeps its
  bytes — TypeScript types the destructuring; no per-call rewrites); template transforms:
  `{#if c}…{:else}…{/if}` → `{c ? (…) : (…)}`; `{#each xs as x, i (key)}` →
  `{xs.map((x, i) => (…))}` (each-without-item per the matrix); `{#await}` → ternary over a
  synthetic promise-state holder; `bind:value={v}` → checkable value/on-change pair; Svelte-5
  event attributes projected VERBATIM lowercase (`onclick` stays `onclick` — typed by
  `SvelteHTMLElements`; the `onClick` rename is RETIRED per D-ae(b); legacy `on:click` carries
  the namespaced attribute verbatim, modifier forms lower through a typed helper against the
  base quoted key — never `onClick`); `{#snippet mySnip(…)}` →
  `const mySnip = __verter_snippet((…) => (…))` (the branded binding) with the D-ap ORDERING
  rule: all snippet declarators of a projected scope function are emitted at the TOP of that
  scope function, in source order, before any sibling content, via CodeTransform MOVE operations
  (bodies keep their original mapped spans) — Svelte snippets are visible to preceding siblings
  in the same lexical scope, and in-place `const` projection would TDZ-error a preceding
  `{@render}` under the clean-type-check gate; `{@render snippet(args)}`
  → `{snippet(args)}` (checks through `Snippet`'s call signature); declaration tags `{const …}`/`{let …}` + legacy `{@const}` → const/let in the
  projected scope function (sibling-run scope-function wrapping per the matrix); `{@html e}` →
  string-accepting checkable position; `{@debug …}` → void reference expression; `{@attach e}` →
  `__verter_attach` checker argument (spread-merged); `class={{…}}`/`class={[…]}` → `ClassValue`
  checker; CSS custom-property attributes (`--track-color={e}`) → STRIPPED from the projected
  JSX attribute position with the value expression void-checked (D-ap — a `--`-prefixed name is
  not a valid JSX attribute identifier; no diagnostic, stable documented Svelte); component
  `<style>` blocks and nested template `<style>` elements → opaque recorded spans, stripped from
  projection (D-ap); `<svelte:element this={e}>` → conservative intrinsic typing;
  `<svelte:window|document|body>` → event-binding projection; `<svelte:boundary>` →
  snippet-machinery projection (`failed`/`pending`/`onerror` typed per the matrix); element typing
  via the D-ae shim's `IntrinsicElements` (`SvelteHTMLElements` — never Vue's intrinsics). Every
  D-ad OUT-OF-SCOPE construct → the matrix's exact behavior
  (parse-without-crash + typed-unsupported diagnostic + void-checked expressions), incl.
  await-expressions (`svelte-await-experimental`).
- **The `@verter/svelte-jsx` shim asset (D-ae, home/distribution per D-av)**: NEW
  `packages/svelte-jsx/` — `jsx-runtime.d.ts` (the D-ae `JSX` namespace), `jsx-dev-runtime.d.ts`
  (re-export), `package.json` (types-only, no runtime JS; `exports` typing `./jsx-runtime` +
  `./jsx-dev-runtime`); published with the other `@verter/*` packages and declared a dependency
  of `@verter/typescript-plugin` (bundled into the VS Code extension with it). The package files
  are the SINGLE hand-written content authority: `verter_session` embeds the d.ts bytes from
  IN-CRATE MIRROR files at
  `crates/verter_session/src/framework/svelte_jsx_assets/{jsx-runtime.d.ts, jsx-dev-runtime.d.ts}`
  (crate-relative `include_str!`; a cross-tree `include_str!` of the package files is FORBIDDEN —
  it would make the byte-pin compare a file against itself and break crates.io packaging, D-av)
  with a byte-pin freshness test (`crates/verter_session/tests/svelte_jsx_shim_freshness.rs`, the
  `typeinfo_proto_ts_freshness.rs` pattern) comparing each in-crate mirror against its
  `packages/svelte-jsx/` canonical — drift fails the gate.
  Locating per consumer (the provider `paths` injection at `configure_paths`,
  `crates/verter_lsp/src/extension_provider.rs:924` — the LSP extension provider is the OWNER
  of provider path injection; the same function sets `jsxImportSource: "vue"` at `:932` — maps
  `@verter/svelte-jsx/jsx-runtime` + `jsx-dev-runtime` to the host-selected location):
  ts-plugin consumers resolve the plugin-bundled package copy via normal node resolution
  relative to the plugin install; provider-inferred projects and TSGO (which reads REAL files —
  virtual content cannot serve it) resolve the host-MATERIALIZED embedded copy, written once per
  host version into the host's own data directory (never into the user workspace); non-VS-Code
  LSP clients and the NAPI/tsserver-ipc consumers ride the same host-owned provider
  configuration. The host-selected copy is AUTHORITATIVE and version-matched to the projection
  the compiler emits — a workspace-installed `@verter/svelte-jsx` is NOT consulted (no
  version-drift class; npm publication exists for explicit user pinning, not as the resolution
  mechanism). **Transitive-dependency resolution (D-ay)**: the shim's OWN imports (`svelte`,
  `svelte/elements`, `svelte/attachments`) cannot resolve from the host data directory or the
  plugin install directory (the node_modules ancestor walk never reaches the user workspace's
  `svelte`; no `baseUrl` rescue for node-style specifiers under `moduleResolution: "bundler"`) —
  the SAME `configure_paths` injection adds per-owner-project `paths` rows mapping `svelte` +
  `svelte/*` to the OWNER WORKSPACE's installed `svelte` package (host-resolved once per owner
  project through the existing workspace package resolution; per-project rows keep
  multi-`svelte` monorepos correct); a workspace with NO `svelte` install gets NO injected
  `svelte` rows and fails CLOSED per D-ae(d).
- LSP: watcher/selector/virtual-file wiring for `.svelte` from the D-x naming column — IDE file
  `<canonical>.tsx` (append semantics: `Foo.svelte` → `Foo.svelte.tsx`) AND the API virtual file
  `<canonical>.ts` (`Foo.svelte.ts` — **shipped in v1**: it is the mechanism a TS file importing
  `./C.svelte` reaches under tsserver, the same role as the live Vue `App.vue.ts`; its CONTENT
  is the B8a-landed D-ak `ComponentApiProjector` declaration shim, served through the unchanged
  `get_public_api` host entry — the existing LSP api sync path (`sync_coordinator.rs:246` etc.)
  carries it with zero call-site changes);
  `sync_coordinator`/`provider_sync` derive BOTH paths from `VirtualFileNaming` (Vue's row pinned
  by a characterization test against the live PRODUCTION derivations —
  `crates/verter_workspace/src/resolver.rs:241/:256` (`provider_id_for_source` /
  `provider_ide_id_for_source`) consumed at `provider_sync.rs:161-162`, plus the inline
  JSX-conditional re-derivation at `provider_sync.rs:350` (`open_unresolved_vue_state`), which
  this cutover routes through the column). VS Code opt-in via
  `verter.frameworks` workspace setting, attaching to the EXISTING `svelte` language id,
  contributing no grammar.
- typescript-plugin: all four Vue-only regex constants
  (`packages/typescript-plugin/src/helpers/utils.ts:1-4` — `DEFAULT_REGEXP = /\.vue$/`,
  `VUE_TS_REGEXP`, `VUE_D_TS_REGEXP`, `VUE_TEST_TS_REGEXP`) plus their consumers
  (`toVueVirtualFileName` :44, `getVueVirtualFileInfo` :15 (the mode-classifying reverse map),
  `isVue` :74, `isVueTs` :77, `isVueTestingTs` :79) generalized to
  the carrier-extension set: one carrier-extension regexp + per-carrier virtual-suffix regexps —
  the api suffix, the TESTING-API suffix where the row declares one (D-al: Vue
  `.__verter_test.ts`; Svelte has NONE — the generated module never forms
  `.svelte.__verter_test.ts`), and the uniform `{carrier_ext}.d.ts` accepted-spelling alias
  (lookup acceptance normalizing to the api surface) — ALL
  derived from the registry's virtual-file naming column (D-x), mirrored as a GENERATED,
  BYTE-PINNED TS constant module `packages/typescript-plugin/src/generated/virtual-file-naming.ts`
  — the Rust registry rows are the single authority; the freshness test
  `virtual_file_naming_ts_freshness` (the `typeinfo_proto_ts_freshness.rs` pattern) renders the
  module from the descriptor table and byte-compares against the checked-in file (regeneration
  via the same test under an explicit update flag), so a hand-edit or a registry-row change
  without regen fails the gate (D-x).

**Legacy Deletions.** The four single-extension Vue regex literals (`utils.ts:1-4`), replaced by
the generated registry-derived constants.

**Tests (failing first).**
1. TSX projection snapshots with NEGATIVE assertions (no `{#if`/`{@render` residue;
   `@ts-expect-error` anti-`any`/`never` guards in typed fixtures).
2. Sourcemap e2e per the `sourcemap_e2e_tests.rs` pattern (hover-position preservation for script
   expressions and template expressions; the prelude is unmapped and shifts no mapped position).
3. **Type-check validity gate (D-u — OXC parse-only is NOT sufficient)**: GATE PRECONDITION
   (D-ae): the TSGO pragma-parity fixture — a `.svelte.tsx`-shaped file whose
   `@jsxImportSource @verter/svelte-jsx` pragma demonstrably overrides the project-level Vue
   setting under BOTH tsserver and TSGO (if TSGO fails it, the named D-ae fallback triggers as a
   STOP-and-redesign, never a silent degrade); then each fixture's projected TSX type-checks
   CLEAN through the TSGO/tsserver path; discriminating rune fixtures —
   `: Props = $props()`, `$props<Props>()`, defaults/rest destructuring, `$bindable`,
   `Snippet<[T]>` children, `$state`/`$derived` in markup — each with `@ts-expect-error`
   anti-`any` guards on a rune-declared prop (a `$props()` member typed `any` fails the fixture);
   D-ad additions under the same gate: an `{@attach}` fixture whose mistyped attachment
   (`Attachment<HTMLInputElement>` on a `<canvas>`) FAILS type-check (discriminating both ways),
   a declaration-tag fixture (`{const}` value typed + visible to a sibling), and a clsx
   `class={{…}}` fixture with an `@ts-expect-error` on a non-`ClassValue` payload; D-ae
   environment discriminators: lowercase `onchange` accepted WITH a typed `currentTarget`
   (`e.currentTarget.value` checks; `@ts-expect-error` on a canvas-only method — proves the
   Svelte intrinsic table is in effect, since Vue/React casing would reject the lowercase
   attribute), `@ts-expect-error` on `onChange` (the retired rename must NOT type-check),
   `onintrostart` accepted (Svelte-specific attribute Vue's JSX rejects — the chosen-table
   proof), a component-tag fixture whose wrong prop is rejected through
   `ElementAttributesProperty { $props }`, snippet-brand both ways (a plain function rejected
   where `Snippet<[T]>` is expected; the `__verter_snippet`-bridged binding accepted; a wrong
   `{@render}` argument rejected), the D-ap snippet-ORDERING fixture — a `{@render mySnip()}`
   PRECEDING its `{#snippet mySnip(…)}` in the same scope type-checks CLEAN (DISCRIMINATING:
   in-place declarator projection fails this with a TS use-before-declaration error), a D-ap
   custom-property fixture (`--track-color={expr}` on an element and a component: no `--`
   attribute residue in the projected TSX — negative assertion — while the value expression
   stays present void-checked with a deliberate type error in it surfacing), and the
   missing-`svelte`-package fixture failing CLOSED with
   the typed `svelte-package-missing` diagnostic (no ambient stub, no `any`). HERMETICITY: the
   gate fixture projects VENDOR the `svelte` package's type declarations locally (`svelte`,
   `svelte/elements`, `svelte/attachments` — version-pinned to the audited 5.56.x per D-ad;
   Testing-Hermeticity MANDATORY — no npm install, no third-party checkout at test time), and
   `paths`-map `@verter/svelte-jsx` directly at the in-repo `packages/svelte-jsx/` (D-av); the
   missing-`svelte`-package fixture is the single DELIBERATE exception (it vendors nothing —
   that is what it tests). ASSET-RESOLUTION case (D-av): one e2e fixture workspace carries NO
   `@verter/svelte-jsx` npm dependency and resolves the shim purely through the provider
   `paths` mapping (DISCRIMINATING: removing the injected mapping fails the fixture with
   module-not-found on the pragma's import source). PRODUCTION-TOPOLOGY case (D-ay): one e2e
   fixture reproduces the deployed topology the other gate fixtures structurally cannot — the
   shim copy is materialized OUTSIDE the fixture workspace (the host-data-directory placement),
   the `svelte` type declarations live ONLY inside the fixture workspace's own `node_modules`
   (vendored in-repo per Testing-Hermeticity — no npm install), and NO `svelte` paths mapping
   exists beyond the rows the D-ay injection itself adds; the projected TSX type-checks CLEAN
   (DISCRIMINATING both ways: removing the injected `svelte`/`svelte/*` rows fails the fixture
   with module-not-found inside the shim's own imports — the exact fixture-green/production-red
   class this fixture exists to catch).
4. LSP e2e fixture (Mocha, hover + go-to-definition through `.svelte.tsx`).
5. typescript-plugin specs for the generalized regexp (`.vue` behavior unchanged — negative
   assertion).
6. D-x/D-al naming characterization against the PRODUCTION derivations: the Vue
   `VirtualFileNaming`
   row reproduces `crates/verter_workspace/src/resolver.rs:241` (`provider_id_for_source` —
   `{canonical}.ts` api) and `:256` (`provider_ide_id_for_source` — `{canonical}.tsx`/`.jsx`
   JSX-conditional ide) byte-for-byte, as consumed at `provider_sync.rs:161-162`, PLUS the
   testing role — `toVueVirtualFileName(f, "testing")` / `VUE_TEST_TS_REGEXP` / the
   `getVueVirtualFileInfo` mode classification (`utils.ts:4/:15-46`) reproduced from
   `testing_api_suffix`, and the `{carrier_ext}.d.ts` accepted-spelling acceptance reproduced
   from the carrier-extension set; the inline
   re-derivation at `provider_sync.rs:350` is cut over to the column (DISCRIMINATING: red if any
   consumer re-derives naming locally instead of reading the column); the structural rule
   `testing_api_suffix.is_some() ⇒ api_suffix.is_some()` unit-pinned; a NEGATIVE pin that the
   generated module forms NO `.svelte.__verter_test.ts` name (Svelte `testing_api_suffix:
   None`); freshness test
   `virtual_file_naming_ts_freshness` byte-pins the generated TS constant module; Svelte
   api-path resolution e2e — a TS file importing `./C.svelte` resolves through `C.svelte.ts`
   under the ts-plugin, served by the B8a/D-ak `ComponentApiProjector` content (instance-export
   and `<script module>`-export resolution asserted through the import), INCLUDING the D-at
   imported-reference case: a `C.svelte` whose `$props()` type is an IMPORTED ALIAS resolves
   through `C.svelte.ts` via the shim's type-only import prelude — the consuming TS file sees
   the correct prop types (tsserver hover/check), the shim text keeps the bare ref un-inlined
   (negative), and no semantic dispatch runs at render time (the static guard's scope).

**Verification.** The gate (TS touched).

**Dependencies.** B8a, B5 (reads the `VirtualFileNaming` descriptor column directly — named
explicitly, not just transitively via B8a → B5) (+B2). Parallel-safe with B8b.

---

### Former B10/B11 — Angular (EXTRACTED, D-ac)

The Angular vertical (facts + surface; templates + TCB sidecar) is EXTRACTED from this program
into **`docs/arch/angular-adapter-program.md`** — a standalone, self-contained follow-up program
carrying the full former-B10/B11 designs (selector-scope facts, `AngularTemplateScopeDb`,
Template IR + TCB contract, the two-stage `ScriptFactProvider` Angular rows,
`input()`/`output()`/`model()` + decorator surface mapping, gated `.html` classification, TCB
sidecar virtual file), its dependencies on the substrate seams this program lands, and an
explicit go/no-go decision (criteria: Svelte vertical landed; Astro reassessment outcome
recorded; the named seam evidence reviewed). Nothing Angular-shaped executes in this program;
`FRAMEWORK_TAG_ANGULAR`, the `.html` gated-candidate row, and the Angular capability bit all
land there (D-aa, D-ac).

---

### B12 — Consumer sweep + docs + final lift (reduced scope, D-ab)

**Context.** Close the program at its re-scoped boundary: route every remaining `.vue`-literal
site through the registry, finish consumer integrations for the REGISTERED adapters (Vue +
Svelte), update owning docs/skills, reconcile the Svelte STOP gate, and emit the exit report
whose evidence package feeds the B7/B9 reassessments and the Angular go/no-go.

**Changes.**
- The `.vue` string sweep: the ~80 remaining non-test `ends_with(".vue")` sites (session, LSP, MCP,
  TS packages) each map to "classification" (registry) or "Vue-adapter behavior" (adapter) — never
  inline reimplementation; `single_language_classifier` allowlist shrunk to the frozen-Vue set.
  (Substrate work — unchanged by the re-scope.)
- MCP framework-aware tools (`crates/verter_mcp/src/` scanner + tools accept any REGISTERED
  adapter id — in this program that set is `vue` + `svelte`; the mechanism is open, no
  per-framework switch); playground Monarch grammar + TypeExplorer for Svelte; unplugin: default
  transform filter STAYS `.vue` (invariant 4 — runtime compile out of scope), meta/typeinfo-facing
  APIs accept any registered framework.
- **STOP-gate reconciliation (Svelte ONLY)**: the U15 block table
  (`crates/verter_session/tests/manifest_data/typeinfo_parity_blocks.rs:50`) lists
  `svelte_adapter_stop_gate_is_registered_out_of_scope` and the analogous `react…` row as
  required guards for planned STOP-gate files. This program supersedes ONLY the Svelte row with
  the real landed adapter — update that U15 row +
  `docs/arch/native-typeinfo-parity-adapters-final-lift.md` to point at the landed registry
  (session-test manifest + docs edit, not typeinfo core). The React STOP-gate row STAYS AS-IS
  (B7 is deferred; its row is superseded only when B7 reopens and lands).
- Docs/skills sweep (reduced to landed scope): `/architecture`, `/compiler-codegen`,
  `/type-resolution`, `/host-session` pointers; the `/framework-adapters` skill finalized
  (substrate + Vue + Svelte + the D-aa tag-semantics rule); CLAUDE.md summary updates name the
  substrate + Svelte scope and point at the deferred section + the extracted Angular program doc.
- **Guard sweep (landed guards only)**: every §6 guard whose block executed is green; guards
  marked DEFERRED/EXTRACTED in §6 are asserted ABSENT from the tree (no stub guard pretending
  coverage — Stub Prevention).
- Program exit report: (i) full known-bug ledger enumeration as the follow-up semantic program's
  input; (ii) the **seam-evidence package** gating the deferred verticals — registry/executor
  behavior, `ScriptFactProvider` two-stage cost + correctness data from the Svelte provider,
  DTO-store multi-adapter behavior, virtual-file/ts-plugin experience, perf-bound deltas — the
  named inputs to the B7/B9 reassessments and the Angular go/no-go (D-ab/D-ac); (iii)
  STOP-and-report any ledger row that turned out to be in-program-fixable but wasn't.

**Legacy Deletions.** Every swept `.vue`-literal branch (enumerated in the block's PR); any
remaining `is_vue_*` helper aliases.

**Tests (failing first).**
1. Widened `single_language_classifier` (red against the pre-sweep tree).
2. MCP/playground integration tests per consumer (Vue + Svelte).
3. Landed-guard sweep: every executed block's §6 guard green; R6 meta-guard green; corpus
   generator idempotent; U15 manifest green with the Svelte row updated and the React row
   untouched.

**Verification.** The gate (TS touched) + a full-program soak on the integration branch.

**Dependencies.** All in-scope blocks (B1–B6, B8a/B8b/B8c).

---

## Deferred Verticals — Evidence-Gated (OUT of execution scope; designs preserved)

Per D-ab these blocks do NOT execute in this program. Deferred ≠ deleted: the designs below are
the binding starting point when each vertical reopens, and no review resolution inside them is
weakened by the deferral. Each is headed by the evidence gate that reopens it. (Angular is not
deferred but EXTRACTED — see the pointer in §8 and `docs/arch/angular-adapter-program.md`.)

### Evidence gate — B7 (React)

**Reopens only on an explicit reassessment after the Svelte proof lands, with evidence from the
landed seams**: B5/B8 registry + executor behavior in production use, `ScriptFactProvider`
two-stage cost/correctness data from the Svelte provider, `FrameworkSurfaceDtoStore`
multi-adapter behavior, the known-bug ledger yield (especially the JSX-namespace /
`Parameters<T>[0]` contracts the B7 probe depends on), and the D-x virtual-file/LSP wiring
experience. The reassessment decides whether B7 executes as designed below, is re-scoped, or
stays deferred.

### B7 — React vertical (descoped, probe-gated)

**Context.** React needs no carrier — `.tsx`/`.jsx` already routes through OXC and the LSP serves
the real file. **Capability ground truth (verified, D-t)**: ALL 9 `typeinfo_tests/jsx.rs` rows are
`#[ignore]`d known-gap contracts (JSX-namespace intrinsic lookup, `Parameters<FC<P>>[0]`, factory
inference), and `Parameters<T>[0]` tuple-index projection is itself an ignored gap
(`indexed_utilities.rs`) — there is NO in-repo proof that React element surfaces resolve today.
B7 is therefore NOT the substrate proof (B5's Vue round-trip parity owns that role); it is a
deliberately minimal vertical whose scope is fixed by a probe, and whose gaps become ledger rows
— never in-program core fixes (§2.12).

**Changes.**
- **Step 0 (REQUIRED, before any adapter code): the capability probe.** Failing-first probe tests
  asserting that (a) a plainly annotated function/arrow component's first-parameter type — read
  from the shallow `FunctionSignature.parameters` `TypeExpr` and resolved via ONE
  `shared_resolve`, NOT via `Parameters<typeof C>[0]` (a registered gap) — and (b) a class
  component's explicit `P` type argument, resolve through existing queries TODAY. Probe outcomes
  bound the v1 PROPS scope; every red shape becomes a known-bug ledger row. If BOTH probe legs
  are red, B7 ships registry row + typed-unsupported surfaces only, React moves behind a named
  follow-up core JSX/React resolution program, and Svelte/Astro proceed regardless (D-t).
- React is SESSION-REGISTRY-ONLY: NO `crates/verter_compiler/src/react/` module is created — a
  carrier-less framework has nothing to register in `verter_compiler`, and an empty `mod.rs`
  would be exactly the stub B6 forbids. React's registration is the session
  `FrameworkAdapterRegistry` row below; if a guard ever needs a React compiler-path scope, it
  names a path allowlist instead of creating a module.
- NEW `crates/verter_session/src/typeinfo/adapters/react/`: `ReactFrameworkAdapter` —
  component selection: selector `(canonical, export_name)` → exported function/class value symbol
  through existing queries. Surfaces (probe-bounded): PROPS = the probe-proven shapes ONLY
  (plain-annotation first parameter; class `P` type arg); `FC<P>`/`ComponentType<P>`-typed,
  factory-inferred, `memo`/`forwardRef`-wrapped components = known-bug ledger rows (unblocker:
  the jsx.rs / indexed_utilities.rs contracts) surfacing as PARTIAL/UNSUPPORTED status with
  diagnostics, never silent empties; SLOTS = typed UNSUPPORTED + known-bug ledger row naming the
  JSX-namespace `ElementChildrenAttribute` contract as the unblocker (NO structural lookup ships
  in B7 — the capability does not exist; the row's discriminating body IS the future structural
  test); EMITS/MODEL/OPTIONS = typed unsupported (callback props REMAIN props); EXPOSE = out of
  v1 (`useImperativeHandle` is flow-dependent — ledger row if a fixture demands it).
- Registry row `"react"` (tag `FRAMEWORK_TAG_REACT`, no carrier, no synth).
- TS: `packages/typeinfo/src/framework-surface.ts` exercised end-to-end + specs extended.

**Legacy Deletions.** None.

**Tests (failing first).**
1. Step-0 probe tests (red-first by definition; their final color SETS the adapter scope).
2. `crates/verter_session/src/typeinfo/typeinfo_tests/react_adapter.rs`: probe-proven component
   shapes, imported components, recursive imports, cache warm hits, `FrameworkSurfaceRequest`
   end-to-end through the executor; per-kind status assertions (SLOTS = UNSUPPORTED with
   diagnostic, distinguishable from supported-empty).
3. Guard/test `react_callback_props_not_emits` (DISCRIMINATING: a fixture with `onClose: () =>
   void` asserts PROPS membership and EMITS absence).
4. Known-bug ledger rows (SLOTS/JSX-namespace, `FC<P>`, factory inference, wrappers as probed) +
   bijection guard green.
5. Vendored fixtures under `crates/verter_session/tests/framework_corpus/react/` + corpus config
   row; hermetic.
6. TS decode spec: React payload → `FrameworkSurface` view (incl. UNSUPPORTED status decode).

**Verification.** The gate (TS touched).

**Dependencies.** B5 (landed substrate). Scheduling is decided at reopen.

---

### Evidence gate — B9 (Astro)

**Reopens only on an explicit reassessment after the Svelte proof lands, with evidence from the
landed seams**: the same seam evidence as B7's gate plus the carrier-specific evidence Svelte
produces (CarrierCompiler ergonomics, eval_source blanking, prelude-based typing, sourcemap e2e
pattern, ts-plugin/api-virtual-file mechanism) — Astro is a second carrier vertical and its
island matrix additionally stresses recursive cross-adapter resolution. The Astro reassessment
outcome is also a named input to the extracted Angular program's go/no-go (D-ac).

### B9 — Astro vertical

**Context.** Frontmatter-fenced carrier with islands. Astro's `Props` is a framework-SPECIFIED
contract read structurally from the frontmatter scope (the macro analog) — not a banned suffix
heuristic — and it has TWO declaration forms, BOTH captured: `interface Props { ... }` AND
`type Props = ...` (alias form, equally documented Astro usage; typed-IR makes capturing both
trivial). Cross-framework islands are the program's recursive cross-adapter stress test.

**Changes.**
- NEW `crates/verter_compiler/src/astro/`: `parser/` producing the **Astro Template IR (D-v)** —
  node kinds `Fragment | Element | ComponentRef | Expression | AttrSpread | SlotOutlet | RawText
  | RawHtml | Comment`; `ComponentRef` stores tag span, resolved frontmatter import binding,
  dotted path, attrs, spreads, children, island directives (`IslandDirective { kind, value_span,
  raw_span }`); uppercase/dotted tags resolve against frontmatter value imports/bindings,
  lowercase = intrinsics, unresolved ComponentRef → typed diagnostic; `---` fences; named
  `<slot>` inventory. `ide/`: frontmatter hoisted as module/function body; template → JSX inside
  an async render function; **`Astro` typing via per-file prelude (D-v)** — `import type
  { AstroGlobal } from "astro"; declare const Astro: AstroGlobal<Props>` (no `Props` →
  `Record<string, unknown>`), unmapped at offset 0; `client:*` directives recorded then STRIPPED
  from projected JSX props (spreads stay JSX spreads after directive filtering).
  `CarrierCompiler` impl + eval_source (frontmatter at raw offsets). **Source-map rules (D-v)**:
  frontmatter 1:1; prelude/render-wrapper/slot-plumbing/`client:*` unmapped; template expressions
  preserve spans; tag/attr diagnostics map to tag/attr spans.
- NEW `crates/verter_semantic/src/analysis/framework_facts/astro.rs` (D-o): the Astro
  `ScriptFactProvider` — gate `FileLanguage::Framework { "astro" }`; captures the
  framework-specified `Props` contract — BOTH `interface Props` and the `type Props = ...` alias
  form — + `Astro.props` usage structurally from the frontmatter scope into the
  `AstroScriptFacts` payload (fixture matrices cover both declaration forms).
- `verter_language` row `.astro`; registry row `"astro"` (tag `FRAMEWORK_TAG_ASTRO` — added by THIS vertical's own wire decision per D-aa, NOT by B1,
  carrier + synth + script-fact provider); `astro_default_synth` (instance surface from `Props`).
- NEW `crates/verter_session/src/typeinfo/adapters/astro/`: PROPS = `Props` members; SLOTS = named
  `<slot>` inventory (payloadless); EMITS/MODEL = typed unsupported (UNSUPPORTED status +
  diagnostic per D-s); EXPOSE = frontmatter exports.
- Cross-adapter islands: NO new invalidation authority (D-v) — `ComponentRef` frontmatter-import
  bindings resolve through existing route facts + each target's adapter synth via the ONE
  `Instantiate` identity; only explicit dependency edges for provider sync.
- LSP/plugin wiring per the B8c mechanism (registry-driven — minimal new code).

**Legacy Deletions.** None.

**Tests (failing first).**
1. Parser/IR + TSX snapshots (+ negatives: no `---` fence residue, `client:*` absent from
   projected props, spread-after-filter correctness, unresolved-component typed diagnostic);
   sourcemap e2e (frontmatter AND template expression positions; prelude unmapped).
2. `typeinfo_tests/astro_adapter.rs` + executor e2e; **type-check gate**: projected TSX
   type-checks clean with `@ts-expect-error` anti-`any` guards on `Astro.props` members (the D-u
   gate pattern applied to the `Astro` prelude).
3. **Island matrix**: `.astro` importing `.vue` + `.svelte` + `.tsx` (React) — each island's
   public type resolves through its own adapter's synth via the ONE `Instantiate` identity;
   recursive and circular cases terminate; named slots + spread attrs across the island boundary.
4. Vendored fixtures `framework_corpus/astro/` + corpus config row.
5. Known-bug rows as surfaced.

**Verification.** The gate.

**Dependencies.** B5 + B6 (landed substrate). Scheduling is decided at reopen.

---

## 9. Per-Framework Surface Mapping (FrameworkSurfaceKind)

Scope markers (D-ab/D-ac): **Vue** and **Svelte** are the EXECUTING columns; **React (deferred)**
and **Astro (deferred)** rows are preserved design material for the evidence-gated B7/B9
reopenings; **Angular (extracted)** is carried authoritatively by
`docs/arch/angular-adapter-program.md` — the column here is a traceability mirror.

| Kind | Vue (reference) | Svelte | React (DEFERRED) | Astro (DEFERRED) | Angular (EXTRACTED) |
|---|---|---|---|---|---|
| PROPS | defineProps/withDefaults | `$props()` type members (incl. snippet-typed); legacy `export let` | probe-proven shapes only (plain-annotation first param; class `P` type arg) — `FC<P>`/factory/wrappers ledgered (D-t) | `interface Props` members (`Astro.props`) | `@Input()` / `input()` / `input.required()` |
| EMITS | defineEmits | legacy `createEventDispatcher<E>` map ONLY (runes callbacks stay PROPS) | unsupported (callback props remain PROPS) | unsupported | `@Output()` / `output()` (`EventEmitter<T>`/`OutputRef<T>` payload) |
| SLOTS | defineSlots | snippet-typed props + legacy `<slot>` | unsupported + known-bug ledger row (unblocker: the JSX-namespace `ElementChildrenAttribute` contract, D-t) | named `<slot>` inventory | `ng-content` select inventory |
| OPTIONS | defineOptions | unsupported | unsupported | unsupported | unsupported (decorator metadata is not an options surface in v1) |
| EXPOSE | defineExpose | exported instance-script members | out of v1 (known-bug row on demand) | frontmatter exports | public class instance surface |
| MODEL | defineModel | `$bindable()` props | unsupported | unsupported | `model()` signals |

Unsupported kinds return TYPED unsupported results — realized on the wire by the D-s per-kind
`FrameworkSurfaceKindStatus` (`UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` + diagnostic; one
entry per known kind), never silent empties and never an unmarked empty `members` list.
No new `FrameworkSurfaceKind` variants are needed for this program; a future framework needing one
performs a schema-versioned wire block per the closed-enum rules.

Per-framework IDE projections, parsing strategies, and synth shapes are specified in their blocks
(B8a/b/c in execution scope; B7/B9 in the Deferred Verticals section; Angular in
`docs/arch/angular-adapter-program.md`). React is adapter-only (no carrier, no synth);
Svelte/Astro get carrier + synth + surface adapter; Angular gets script-facts + surface adapter
and a template carrier + TCB sidecar (extracted program).

---

## 10. Commit / Squash Policy

- WIP commits are free during a block (scratch state, `todo!()` allowed pre-squash per the WIP
  exemption).
- Each block lands as **ONE squashed conventional commit** on the integration branch
  (`feat/framework-adapters`, D-e) after: green canonical gate + dual review (independent reviewer
  + codex) + clean re-review on fixes.
- Final commit messages: conventional type/scope, **no phase/temporal vocabulary** (no block
  numbers, no "phase", no "cutover stage"). CLAUDE.md scopes have no framework entries — use
  existing scopes: `core` (verter_compiler), `lsp`, `meta`, `types`, `ts`, `napi`, `wasm`,
  `unplugin`, `play`, or `*` for multi-area blocks. Examples:
  `feat(*): add framework adapter registry and framework-surface executor`,
  `feat(core): add svelte carrier parser and IDE TSX projection`.
- Co-authored-by trailer per repo convention. The orchestrator owns git; implementer sub-agents do
  not push.

---

## 11. Vue Alignment — Future Work (explicitly out of this program)

1. `TypeInfoSurfaceMember.declared_in_macro_type_arg`
   (`crates/verter_session/src/typeinfo/surface.rs:~145`) → structured framework-neutral
   `SurfaceMemberProvenance` enum (in-program: doc fixed to its neutral meaning; no wire change).
2. `ReductionDemand::MacroObjectSurface` (`semantic_query.rs:~808`) — neutral rename; evaluate
   per-framework demand variants if a framework needs a non-Vue union convention.
3. `SemanticNodeData::VueMacroElements` + `HostResolvedNamedTypeKey` (`semantic_query.rs:4374,
   4843`) — generalize only if a framework grows a macro-elements-cache analog; until then new
   adapters must NOT add parallel resolver sidecars.
4. `ParsedSfc` schema generalization + moving Vue natively onto `CarrierCompiler` (retiring
   `vue_bridge.rs` delegation). (The `VueShallowMetadataStore` → `FrameworkSurfaceDtoStore`
   migration is NOT future work — it lands in B5 per D-p.)
5. Vue IDE-path alignment (the Vue IDE template walker vs the framework_common TSX-projection
   helpers).
6. Macro-recognition generalization (adapter-supplied macro table — rejected rev2 D6, see D-d).
7. The U14 component-meta cutover: `packages/component-meta/src/framework-adapter.ts` +
   TS `FrameworkAdapterRegistry`, consuming this program's Rust registry as semantic authority;
   `ComponentMetaAnalysis` generalization (fallthrough analogues: Svelte `$$restProps`, etc.).

---

## Appendix A — Considered Alternatives (rejected, recorded)

- Closed `CarrierKind`/`ParsedCarrier` enums for routing + parse payloads (co-architect) — rejected
  by the lead follow-up: contradicts open `framework_adapter_id`; every future framework would
  require central enum edits.
- `verter_carrier` crate name (rev2 D1) — `verter_language` chosen; the crate classifies all
  files, not only carriers.
- Re-export-alias-only treatment of `AnalyzedExternalTypeSource` (lead's pre-relaxation verdict,
  co-architect §1.6) — superseded by FQ1 physical rehoming under the relaxed constraints.
- Single-method `resolve_surface_kind` adapter trait (co-architect) — lead's two-phase
  plan/normalize wins; the capability-scoped context idea is retained. (The co-architect's
  `verter_session/src/framework/` module home was later partially ADOPTED for the registry
  substrate per D-q; only the surface-adapter impl placement keeps the lead's
  `typeinfo/adapters/` home.)
- Adapter-supplied macro table in the shared analyzer (rev2 D6) — future work (D-d).
- TS-side `framework-adapter.ts` registry in-program (rev2 D10) — deferred to U14 (D-i).
- Stacked branches directly on `main` now (codex) — not viable while `main` lags 2355 commits
  behind the substrate branch; adopted as the post-overhaul-landing model (D-e).
- Per-framework `ComponentMetaAnalysis` clones (dossier §5.1 implication) — rejected as a second
  metadata pipeline; `FrameworkSurfacePayload` is the cross-framework carrier (S2).
- Verter-authored runtime compilers for Svelte/Astro/Angular — out of scope (S1); official
  compilers remain the runtime authority.

*End of plan.*
