# Native Typeinfo Parity — Framework Adapters / Integrations / Final Lift

Parent architecture: docs/arch/native-typeinfo-parity.md
Sequencing authority: docs/arch/semantic-db-overhaul-unified-remaining-plan.md
Owning U-block(s): U14, U15
Prerequisites: U0 (the manifest / ledger / coverage substrate that gates every block's row-lift via the manifest guard suite, with the §10.4 coverage gate enforcing at `U13.PROJECTION` / `U15.FINAL_LIFT` — `docs/arch/native-typeinfo-parity-u2-reducers.md::U0.MANIFEST_SUBSTRATE`); U2 (the whole reducer + typed-value-domain + `SemanticQueryKeySpec` parent, including `U2.JSX_FOUNDATIONS` which owns the nine `jsx.rs` JSX rows — `docs/arch/native-typeinfo-parity-u2-reducers.md`); U6 (the demand-sliced flow / call solver — `docs/arch/native-flow-return.md`); U8 (the wire-surface closure), U10 (the result DB + mode/demand exactness), U11 (the public relation / session surfaces), U13 (the published `GraphTypeNode` + TS `TypeDescriptor` projection — `docs/arch/native-typeinfo-parity-cache-export-session.md`). U1 / U4 / U5 (the persistent cache-runtime node substrate, the scheduler DAG, the artifact-store rehoming) are non-parity prerequisites depended upon, not owned here.
Consumers: the LSP (hover → graph+display, completion → framework surface), MCP (`typeinfo.*` / `component-meta.*` tools), `@verter/component-meta` (native + compat), unplugin, and the playground type explorer — every host-backed consumer reads the U13 published projection + the U14 framework-surface adapter. U15 is terminal: it has no downstream parity consumer; it is the final-lift block the whole parity effort converges on.
Progress ledger: crates/verter_session/tests/typeinfo_ignored_test_manifest.rs

---

## Scope and authority

This child subplan owns the **terminal half** of the native-parity engine — the
framework-surface adapter that projects the engine's published surface into the
Vue (and the STOP-gated Svelte/React) component-meta payload, the host
integrations that expose the surface to the LSP / MCP / playground, and the
**final lift** that drives every remaining `IgnoredTestRow` to `Lifted` — and
cites, never restates, the parent for the engine architecture.

It owns the parity blocks landing in **U14, U15**:

1. **U14** — the framework macro / composite adapter surface over the graph: the
   rebuild of `@verter/component-meta` as a thin `FrameworkSurfacePayload` adapter
   + `FrameworkAdapterRegistry`, with `compat` a projection wrapper (no semantic
   recovery), reading the U13 published `TypeDescriptor` projection structurally.
   It lifts the single `MacroResolution` row (the framework-macro graph adapter)
   and fixes the known Vue mismatch cases. Parent authority: PART 1 §8 (JSX through
   existing queries), the Component-Meta Native vs Compat Rule, the Typed-IR-Only
   Resolver Rule, the Macro-Type-Traversal Rule.

2. **U15** — the **final lift**: the host integrations (LSP / MCP / playground),
   the composite end-to-end adapter surfaces, the Svelte/React STOP-gates, the
   typed bench schema, the final find-grep sweep, and the **terminal acceptance**
   — every `IgnoredTestRow` `Lifted` (zero stale ignored rows except the explicit
   STOP-gates), every `AdditionalProofRow` covered, the unified §9 terminal
   acceptance satisfied for the parity scope, and a green full workspace (CI) gate.
   Parent authority: the Capability Map → "The guarantee over the 362 rows", PART 2
   §§10–14 (the two-table ledger, the coverage table, the git/CI landing protocol, the
   no-skip guarantee, the resume protocol).

The parent is the architecture authority. Each block below cites the parent
section that defines the architecture it implements and states only the concrete
**block contract** — what changes, what is deleted, which named guards land, which
exact manifest rows lift, and how it is verified. No block restates the engine
spec, the wire-purity closure, the per-key cache-soundness rules, the typed
value-domain, or the budget contracts; those live in the parent and are referenced
by section number.

Every block contract uses the parent's per-block contract template (PART 2 §9).
"Done" for any block is the parent's done predicate (PART 2 §11.5 / §11.7 — its
`Typeinfo-Block:` trailer merged + rows `Lifted` + required guards present); a
block's row-lift is gated by the manifest guard suite + the landed §10.3 proof
rail, with the §10.4 coverage gate (complete + non-placeholder coverage)
enforcing from the `U13.PROJECTION` / `U15.FINAL_LIFT` landings onward and
covering earlier-lifted rows retroactively (PART 2 §10.4); landing is the git/CI protocol — branch per block →
green CI → three-reviewer LAND → squash-merge with the `Typeinfo-Block:` trailer
(PART 2 §§11–14). None of that machinery is re-specified here.

### Block dependency graph (within this subplan)

```
U13.PROJECTION done  +  U11.PUBLIC_RELATION_SESSION done   (the published surface + session surface; not owned here)
        │
        ▼
U14.MACRO_ADAPTER   (thin FrameworkSurfacePayload adapter + registry; compat projection; the MacroResolution row)
        │
        ▼
U15.FINAL_LIFT      (LSP/MCP/playground integrations + CompositeSurfaces rows + STOP-gates + bench schema +
                     final sweep + TERMINAL ACCEPTANCE over all 362 IgnoredTestRows)
```

`U14.MACRO_ADAPTER` consumes the U13 published `GraphTypeNode` / `TypeDescriptor`
projection: it cannot adapt a surface that is not yet published, so it declares the
whole U13 (and transitively the U2 / U6 / U8 / U10 / U11 / U12 / U13) chain a
prerequisite. `U15.FINAL_LIFT` is the **terminal** block — it depends on every
code-producing block (U0–U14) and runs last; it lands the composite end-to-end
surfaces over the U14 adapter and the U2/U6 reducers, the host integrations, and
the final acceptance that confirms every parity row is `Lifted`.

### Row ownership (no double-counting)

These two blocks own exactly the rows whose coverage `block_id` is one of their
blocks; no row is owned twice with any other subplan (Capability Map; PART 2
§§10.4–10.5). The split, stated explicitly so the binding 362 `IgnoredTestRow`
total stays exact:

- **`MacroResolution` (1)** — the single framework-macro graph-adapter row
  (`basic.rs::component_like_slot_payload_extracts_parameters_from_nested_slot_property`)
  lifts in **U14.MACRO_ADAPTER**.
- **`CompositeSurfaces` (5)** — all five end-to-end adapter-surface rows
  (`menu_like.rs` ×2, `message_list_like.rs` ×2, `table_like.rs` ×1) lift in
  **U15.FINAL_LIFT**.
- **`JsxResolution` (9)** — the nine `jsx.rs` rows are owned by
  `docs/arch/native-typeinfo-parity-u2-reducers.md::U2.JSX_FOUNDATIONS` (the JSX
  resolution foundation over the U2 reducer substrate), **NOT** here. U14's
  framework adapter CONSUMES JSX component resolution as a published foundation
  (class-component and function-component element surfaces resolve through the
  normal `ResolveClassSurface` / signature surfaces and project through
  `IndexedAccess` / `KeyOf` — parent §8), but it lifts no `JsxResolution` row; the
  six JSX no-new-key submatrix `AdditionalProofRow`s are likewise registered in
  `U2.JSX_FOUNDATIONS`, not here. This subplan adds no `JsxResolution` row and no
  JSX `AdditionalProofRow`.

These two blocks therefore lift exactly **6** `IgnoredTestRow`s combined
(`MacroResolution` 1 + `CompositeSurfaces` 5). In every case the parent's §10.4.1
partition (the HAND-AUTHORED row→`block_id` map the generator parses — distinct from
the PART 2 §10.4 row-exact coverage table, which is not yet built and is
forward-declared on the `U13.PROJECTION` / `U15.FINAL_LIFT` block-contract rows) is
the authority: each row
maps to exactly one `block_id` via its `mechanism_id`, and
`capability_rows_map_to_expected_query_fact_mechanisms` (gated at the `U13`/`U15`
landings) asserts the mapping is
consistent with the capability. The binding 362 total stays exact.

---

# U14 — Framework macro / composite adapter

## U14.MACRO_ADAPTER

ID: U14.MACRO_ADAPTER
Parent U-block: U14
Subplan: docs/arch/native-typeinfo-parity-adapters-final-lift.md

Prerequisites: U13.PROJECTION, U11.PUBLIC_RELATION_SESSION, U10.RESULT_DB, U8.WIRE_SURFACE_CLOSURE, U2 (parent), U6 (parent).
Blocked until: U13.PROJECTION done (the adapter reads the published `GraphTypeNode` / TS `TypeDescriptor` projection structurally) and U11.PUBLIC_RELATION_SESSION done (the adapter consumes the public session surface + the request footprint), and the whole U2 + U6 parents done (the macro surfaces it adapts — `defineProps` / `defineEmits` / `defineModel` / `defineSlots` / `withDefaults` payloads and the imported `.vue`-component surfaces — resolve through the U2 reducers and the U6 flow / call solver). This block is the framework-adapter that U15's composite surfaces build on.

Context: The native component-meta payload is the semantic authority and `@verter/component-meta/compat` is a projection layer, not a second semantic pipeline (the Component-Meta Native vs Compat Rule). Today the native component-meta resolution path carries framework-specific resolution that must be cut over to a thin `FrameworkSurfacePayload` adapter + a `FrameworkAdapterRegistry`, so the framework surface is a STRUCTURAL projection of the engine's published results (the U13 `TypeDescriptor` projection) rather than a per-macro engine. Macro resolution is one shared path, not a per-macro engine (the Macro-Type-Traversal Rule): every macro and every imported `.vue`-component surface resolves through exactly TWO steps — resolve ONE type via the shared typed-IR five-mode dispatch, then normalise per kind (a thin transform, not a resolver). The single `MacroResolution` row exercises this: `Parameters<NonNullable<T['slot']>>[0]` is a cross-file slot-payload extraction that today stops behind a `semanticMiss` indexed access; lifting it routes the slot-payload type through the shared `IndexedAccess` / `Instantiate` / flow-call reductions (`Parameters<…>` is a U2 intrinsic over the U6-resolved call signature) and normalises the slot binding — NOT a macro-specific walker. The four known Vue mismatch cases (Popover `SlotProps<M>`, theme-alias display, `Button["variants"]["color"]` indexed-access, ContentSearch intersection) are fixed in the NATIVE layer first (fix metadata in the native layer first; Rust owns resolution, declaration routing, and graph construction). This block exists now because the composite end-to-end surfaces (U15) are built on the framework adapter, and the adapter must be a thin structural projection before the composite rows can lift end-to-end.

Changes (exact files / functions):
- `crates/verter_session/src/typeinfo/surface.rs` (the `FrameworkSurfacePayload` projection driven by `TypeInfoSurface::build`) — project the engine's published `TypeInfoGraphPayload` (the U13 `GraphTypeNode` type-value surface + the relocated side tables) into the `FrameworkSurfacePayload` carrier that embeds `TypeInfoGraphPayload` (the additive `TypeInfoGraphPayload` carrier U8 added at the next free tag, NOT the retired `FrameworkSurfacePayload.graph = 4` embedding — PART 1 §1.5). The framework surface is a structural projection of the engine's typed results; it does no query-time resolution.
- `packages/component-meta/src/compat/native-projection.ts` (the existing `decodeComponentMetaPayload` consumer; the decoder itself lives in `packages/component-meta/src/type-graph-decode.ts`) + a NEW `packages/component-meta/src/framework-adapter.ts` (the `FrameworkAdapterRegistry` + the Vue adapter — no such registry exists in the tree yet) — rebuild `@verter/component-meta` as a thin adapter over the `FrameworkSurfacePayload`: the registry selects the Vue adapter, the adapter projects the published `TypeDescriptor` surface (props / emits / slots / expose / model) into the native component-meta payload STRUCTURALLY (reading `prop.type` (`TypeDescriptor`), never `prop.rawType`), and normalises per macro kind (props: defaults / optionality / readonly / provenance; emits: call-signature event extraction first; slots: function-like members, first-param object → bindings; options/expose: pass-through). No second resolver/expander, no AST/source fallback (cache-owned type recovery only — Component-Meta Native vs Compat Rule).
- `packages/component-meta/src/compat/checker.ts` + `packages/component-meta/src/compat/schema.ts` — keep `compat` a projection wrapper over the native payload for `vue-component-meta` interop: it transforms STRUCTURE but must not recover MEANING; every semantic decision reads `prop.type` (`TypeDescriptor`), `prop.rawType` is display passthrough only, and type-role classification is structural (a type is a prop/emit/model/slot type because a Vue macro consumes it, not because its identifier ends with `"Props"` / `"Emits"` / `"Model"` / `"Slots"`).
- `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` + the slot-payload projection path — route `Parameters<NonNullable<T['slot']>>[0]` through the shared reductions: resolve `T['slot']` via `IndexedAccess`, strip `null`/`undefined` via the `NonNullable` intrinsic, take the call signature's parameters via `Parameters` (the U2 intrinsic over the U6-resolved call signature), index `[0]`, and normalise the slot binding — removing the `semanticMiss` indexed-access stop so the terminal slot-payload type resolves.
- `crates/verter_session/src/typeinfo/typeinfo_tests/` Vue mismatch fixtures + `packages/component-meta/tests/` — the four mismatch-case regression fixtures (Popover `SlotProps<M>`, theme-alias display, `Button["variants"]["color"]` indexed-access, ContentSearch intersection), each fixed in the native layer and each failing on the legacy native-component-meta path / passing on the rebuilt adapter.

Deliverables:
- `@verter/component-meta` rebuilt as a thin `FrameworkSurfacePayload` adapter + `FrameworkAdapterRegistry`, projecting the U13 published `TypeDescriptor` surface structurally into the native component-meta payload; `compat` a projection wrapper that transforms structure only.
- The `MacroResolution` slot-payload extraction (`Parameters<NonNullable<T['slot']>>[0]`) routed through the shared `IndexedAccess` / `NonNullable` / `Parameters` reductions + the U6 call signature, with the `semanticMiss` indexed-access stop removed.
- The four Vue mismatch cases fixed in the native layer (Popover `SlotProps<M>`, theme-alias display, `Button["variants"]["color"]` indexed-access, ContentSearch intersection), each with a legacy-fails / rebuilt-passes regression test.

Legacy deletions:
- The legacy native-component-meta resolution path the `FrameworkSurfacePayload` adapter replaces — cut over, NOT dual-pathed (the rebuilt adapter is the only framework-surface projection; no second framework resolution path survives).
- Any per-macro / per-surface framework resolver in the native or compat layer (folded into `shared_resolve(type) + normalise` — the Macro-Type-Traversal Rule; a macro/import resolving its surface through anything other than the shared resolver is removed).
- Any compat branch that recovers MEANING from `prop.rawType` / a raw / display string (`looksLike*` / `extract*` / `normalize*` / `split*` / `strip*` / `prefer*` / `shouldPrefer*` / `repairOpaque*`) or any hand-rolled type-text splitter — replaced by reading `prop.type` (`TypeDescriptor`) structurally (the Typed-IR-Only Resolver Rule; the `@verter/component-meta` no-rawtype-reads contract).
- Any identifier-name-suffix type-role classification (`name.ends_with("Props")` etc.) in the adapter / compat layer — replaced by structural macro-participation classification (`AnalyzedMacro.kind` / `parsed_type_argument` / `type_references`).
- Any `semanticMiss` indexed-access stop on the `Parameters<NonNullable<T['slot']>>[0]` slot-payload path — replaced by the shared-reduction resolution.
- No projection-repair / second-engine path remains in the framework adapter or the compat layer. Stated explicitly per the template.

SemanticQueryKey/facts touched: no NEW key; the adapter READS the published `TypeInfoGraphPayload` (the exporter's output, decoded as `TypeDescriptor`) and projects it into the framework surface. The `MacroResolution` slot-payload extraction dispatches the existing reducer keys — `IndexedAccess` (the `T['slot']` hop and the `[0]` element), `Instantiate` (the `NonNullable` / `Parameters` intrinsics — `IntrinsicRegistry::lookup`), `ResolveCall` / `FlowReturn` (the U6 call signature `Parameters<…>` reads), `ProjectPath` (the terminal projection). Facts read: `Member` / `MemberPresence`, `RouteGeneration` / `ExportSurface` (the cross-file `.vue` / barrel import graph), `LibIntrinsic`, `TypeEnvOptions`, project-generation facts. Admission: inherits the budgets of the dispatched keys (`KeyspaceBudget` for the indexed-access hops, `CallResolutionBudget` for the call-signature read); the adapter itself does no admission (it projects an already-admitted payload).

Exact test rows lifted (capability `MacroResolution`, `basic.rs`):
- basic.rs::component_like_slot_payload_extracts_parameters_from_nested_slot_property

(The nine `jsx.rs` `JsxResolution` rows and the six JSX no-new-key submatrix `AdditionalProofRow`s are owned by `docs/arch/native-typeinfo-parity-u2-reducers.md::U2.JSX_FOUNDATIONS`, NOT this block — U14 consumes JSX component resolution as a published foundation but lifts no `JsxResolution` row. The hand-authored parent §10.4.1 row→block_id partition assigns each row to exactly one `block_id` (the §10.4 generated coverage table — the U13/U15-gated unbuilt residual — is checked against it when it lands); no `JsxResolution` row is double-counted here.)

Required new guards (PART 1 §8; the Component-Meta Native vs Compat Rule):
- `component_meta_is_thin_framework_adapter_no_second_resolver` — asserts `@verter/component-meta` is a thin `FrameworkSurfacePayload` adapter with no second resolver / expander (cache-owned type recovery only); the framework surface is a structural projection of the published payload, not a re-resolution path.
- The four mismatch-case regression tests (each fails on the legacy native-component-meta path, passes on the rebuilt adapter): Popover `SlotProps<M>`, theme-alias display, `Button["variants"]["color"]` indexed-access, ContentSearch intersection.

Critical-rule guards: this block implements the parent's `(CRITICAL)` Component-Meta Native vs Compat Rule, the Macro-Type-Traversal Rule, and the Typed-IR-Only Resolver Rule (the framework adapter projects the published typed surface structurally; the compat layer recovers no meaning from display strings; macro resolution is one shared path). The thin-adapter + no-rawtype-reads + structural-classification guards are these rules' R6 guards: the `@verter/component-meta` no-rawtype-reads contract (`packages/component-meta/tests/no-rawtype-reads.spec.ts`), the published-surface parity (`crates/verter_audit/tests/published_surface_constants_match_ts_port.rs`), and the architecture-guard list for the typed-IR-only pipeline. This block must not regress them. Any new `(CRITICAL)` framework-adapter rule text added to docs registers its guard here in the same change.

Proof requirement: per-row — the `MacroResolution` row is `Ts7Oracle` (the exact TS judgement of the resolved `Parameters<NonNullable<T['slot']>>[0]` slot-payload type) paired with the structural assertion that the terminal slot-payload resolves through the shared reductions without a `semanticMiss` (`OracleAndGuard`). Consumed by the row's §10.3 proof-consumption rail (PART 2 §10.3; landed shape: the registry-bound driver-calling row body). The four mismatch-case regression tests are `StructuralGuard`-class (legacy-fails / rebuilt-passes), and the thin-adapter / no-rawtype contract is pinned by the existing no-rawtype-reads + published-surface-parity tests.

Exit acceptance:
- The `MacroResolution` row lifts and passes on the normal `lib*.d.ts` corpus; `Parameters<NonNullable<T['slot']>>[0]` resolves the terminal slot-payload precisely (no `semanticMiss`).
- `@verter/component-meta` is a thin `FrameworkSurfacePayload` adapter (the thin-adapter guard green); `compat` transforms structure only; every semantic decision reads `prop.type`, `prop.rawType` is display-only (the no-rawtype-reads guard green); type-role classification is structural.
- The four Vue mismatch cases pass on the rebuilt adapter and fail on the legacy path (the four regression tests green); the legacy native-component-meta resolution path is gone (no dual path).

Verification commands:
- `cargo test --package verter_session` typeinfo surface / framework-surface / slot-payload tests.
- `pnpm vitest --run packages/component-meta/tests/no-rawtype-reads.spec.ts` and the `@verter/component-meta` native-projection / framework-adapter / compat checker+schema spec suites (incl. the four mismatch-case regression specs).
- `cargo test --package verter_audit --test published_surface_constants_match_ts_port` (Rust/TS published-surface parity).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (manifest guard suite for this block's row).
- The block's lifted-row proof via the generated wrapper (or `cargo test … -- --ignored` before the branch strips the `#[ignore]`s).
- The full workspace gate (the CI gate — the complete Rust **AND** JavaScript gate, green only when BOTH pass; PART 2 §11.2): `cargo nextest run --workspace` + `cargo test -p verter_session --tests` (the canonical Rust pair — bare `cargo test --workspace --tests` silently skips the verter_session integration suite and is NOT the gate); `cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`; `pnpm test`; `pnpm install --frozen-lockfile`.
- `node scripts/gen-corpus-audit-tests.mjs` (idempotent; if audit-record schema/fixtures change).
- Commit cadence / review gate: PARENT-UNIFORM — the uniform discipline for EVERY block in this subplan (parent PART 2 §11.11 / §11.12), stated once and not restated per block: each block lands as ONE squashed commit (WIP series during the work, no per-commit gate) after the three-reviewer LAND verdict (1 Claude Code + 2 codex).

Docs updated: keep the `/component-meta` native-vs-compat + framework-adapter-registry sections current (the rebuilt `FrameworkSurfacePayload` adapter + `FrameworkAdapterRegistry`); update the `/architecture` framework-surface notes if the registry surface changes.

Re-entry notes: idempotent. If partial, the manifest shows whether the `MacroResolution` row still carries `#[ignore]`. The framework adapter is a thin structural projection — if a compat branch reaches for `prop.rawType` to recover meaning, the no-rawtype-reads guard fails; fix the producer (add the missing `TypeDescriptor` variant) rather than parsing text. Do not reintroduce the legacy native-component-meta resolution path or a per-macro walker.

---

# U15 — Final lift (terminal acceptance)

## U15.FINAL_LIFT

ID: U15.FINAL_LIFT
Parent U-block: U15
Subplan: docs/arch/native-typeinfo-parity-adapters-final-lift.md

Prerequisites: U14.MACRO_ADAPTER, and every code-producing parity block (U0–U14): U0 (manifest/ledger/coverage/cutover substrate), the whole U2 parent (reducers + typed value domain + `SemanticQueryKeySpec`), the whole U6 parent (flow / call solver), U3 (cache / fact model), U8 (wire-surface closure), U10 (result DB + mode/demand exactness), U11 (public relation / session surfaces), U12 (exporter), U13 (published projection), U14 (framework adapter).
Blocked until: U14.MACRO_ADAPTER done (the composite surfaces are built on the framework adapter) AND every other code-producing parity block done (this is the terminal block — it lifts the last rows over the full stack and asserts the whole-effort terminal acceptance). A parent U-block token is never landed while any child block's rows remain `Ignored` — i.e. while any child block's `Typeinfo-Block:` trailer is unmerged (PART 2 §11.9); U15 is the block where the final parent tokens resolve.

Context: This is the **final lift** — the terminal block the whole native-parity effort converges on (unified §U15: "Integrations, Ignored-Test Lift, Bench Schema", deps = all code-producing blocks U0–U14, runs last). It owns three things. (1) The host integrations that expose the published surface to consumers — Zod/schema client helpers, LSP hover → graph+display and completion → framework surface, the MCP `typeinfo.*` / `component-meta.*` tools, and the playground type explorer — each a thin consumer of the U13 published projection + the U14 framework adapter, NOT a second resolver. (2) The composite end-to-end adapter surfaces (`CompositeSurfaces`, 5 rows): nested-conditional-utility model values, slot-payload extraction with model context, `Pick` from an inferred array element, payload remap with message context, and dynamic-slot template-literal-key projection — each an end-to-end exercise of the U2 reducers + the U6 flow/call solver + the U14 framework adapter over a realistic component-like fixture, lifting through the shared engine with no composite-specific resolver. (3) The **terminal acceptance** itself — the mechanical "done" for the whole parity effort: every `IgnoredTestRow` is `Lifted` (zero stale ignored rows, the only permitted residual `#[ignore]`s being the explicit Svelte/React STOP-gate files), every `AdditionalProofRow` is covered and non-placeholder, the typed bench schema is in place, the final find-grep sweep removed every remaining legacy entry-point name, and the full workspace gate is green. This block exists now because it is the last block in the dependency graph: it lands the integrations + composite surfaces over the complete stack and is where the unified §9 terminal acceptance for the parity scope is satisfied.

Changes (exact files / functions):
- `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` + the composite-surface projection paths (the `menu_like` / `message_list_like` / `table_like` fixtures' resolution) — route each composite surface end-to-end through the shared reductions + the U14 framework adapter: nested conditional utilities over a model value (`Instantiate` / `Conditional` / `IndexedAccess`), slot-payload extraction with item + model context (`Parameters` / `NonNullable` / `IndexedAccess` + slot normalisation), `Pick` from an inferred array element (`IndexedAccess` over the array element type + the `Pick` intrinsic), payload remap with message context (slot first-parameter object → bindings), and dynamic-slot projection over template-literal keys (`TemplateLiteralReduce` → `KeyOf` → `IndexedAccess`). No composite-specific resolver.
- `packages/component-meta/src/adapters/zod.ts` + `packages/component-meta/src/adapters/json-schema.ts` (the existing Zod / JSON-schema client adapters) and `packages/component-meta/src/compat/schema.ts` — the schema client helpers that project the published `TypeDescriptor` surface (decoded via `packages/component-meta/src/type-graph-decode.ts` / `type-graph-proto-decode.ts`) into the client schema (a structural projection of the published payload).
- `crates/verter_lsp/src/features/hover.rs` + `crates/verter_lsp/src/features/completion.rs` (the LSP hover → graph+display and completion → framework-surface providers) — wire hover to the published `TypeInfoGraphPayload` (graph + display) and completion to the `FrameworkSurfacePayload`, each a thin consumer of the U13 projection + the U14 adapter through the shared host path.
- `crates/verter_mcp/src/server.rs` (+ a new `crates/verter_mcp/src/tools/typeinfo.rs` module) — register the MCP `typeinfo.*` / `component-meta.*` tools that expose the published surface + the framework adapter (one async native request per query; no second resolver in the tool layer). The `crates/verter_mcp_server/src/main.rs` binary mounts them unchanged.
- New file: `packages/playground/src/components/TypeExplorer.vue` (the playground type-explorer surface) — reads the published `TypeInfoGraphPayload` structurally; wired into `packages/playground/src/App.vue`.
- New files: `crates/verter_session/src/typeinfo/typeinfo_tests/svelte_adapter_stop_gate.rs` + `crates/verter_session/src/typeinfo/typeinfo_tests/react_adapter_stop_gate.rs` (registered in `crates/verter_session/src/typeinfo/typeinfo_tests/mod.rs`) — the Svelte/React STOP-gate files (the explicit out-of-scope adapters whose `#[ignore]`s are the only permitted residual ignored sites after the final lift; they are STOP-gates, not parity rows in the 362).
- New files: `packages/benchmark/src/cache-runtime-bench.ts` (`BenchResultRow`) + the vendored component-meta corpus benches (`component_meta_cold` / `component_meta_warm`, alongside the existing `packages/benchmark/src/component-meta-artifact.ts`) + `crates/verter_session/src/test_support/timeout.rs` (`MAX_TEST_TIMEOUT`, in a new `test_support` module registered in `crates/verter_session/src/lib.rs`) — the typed bench schema reporting cache mode / source-map policy / batch shape / thread count / hit count / fallback count over VENDORED corpora (Testing-Hermeticity: no `.integration-tests/repos/<third-party>/`).
- New files (the PART 1 §6.2 performance-contract benches — perf-regression-gated terminal acceptance): `packages/benchmark/src/verter-vs-tsgo-bench.ts` — the **Verter-vs-TS/tsgo benchmark fixtures** running Verter and TS/tsgo over the SAME semantic queries (component-meta resolution, projected typeinfo, IDE hover/completion queries, selected member expansion — `Pick`/`Omit`/a single demanded member off a large surface, and the `ReturnType<typeof f>["b"]` demand-slice case), each reported with the `BenchResultRow` contract (cache mode / source-map policy / batch shape / thread count / hit count / fallback count) so the comparison is apples-to-apples; and `crates/verter_session/src/test_support/perf_contract.rs` (registered in the `test_support` module) — the **per-family fallback-bound benches** declaring each query family's (`FlowReturn` / `ResolveCall` / `Relate` / `Instantiate` / `Conditional` / `MappedType` / `ResolveClassSurface` / `ApparentType` / `TemplateLiteralReduce` / the projection·demand-lattice families) fallback-count bound on the vendored corpus and FAILING the bench when a family's `BenchResultRow.fallback` count exceeds its bound (the governing-rule metric: fallback ENTRY rate, not fallback latency). All over VENDORED corpora (Testing-Hermeticity).
- `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` — the terminal-acceptance assertions over the live ledger: the `IgnoredTestRow` table is exactly 362 rows, every row's `status == Lifted` (zero `Ignored`), `EXPECTED_TOTAL_IGNORED_COUNT == count(status == Ignored) == 0` over the parity rows, the source-`#[ignore]` ↔ `Ignored` bijection holds with the only residual source `#[ignore]`s being the STOP-gate files, every `AdditionalProofRow` resolves to a non-placeholder mechanism + executable proof, and no parent/aggregate U-block token is vacuously landed.

Deliverables:
- The host integrations: Zod/schema client helpers, LSP hover → graph+display + completion → framework-surface, the MCP `typeinfo.*` / `component-meta.*` tools, and the playground type explorer — each a thin structural consumer of the U13 projection + the U14 framework adapter.
- The five `CompositeSurfaces` end-to-end adapter surfaces, each resolved through the shared U2 reducers + U6 flow/call solver + the U14 adapter (no composite-specific resolver).
- The Svelte/React STOP-gate files (the explicit out-of-scope residual `#[ignore]`s) and the typed bench schema (`BenchResultRow` + vendored cm corpus benches + `MAX_TEST_TIMEOUT`).
- The PART 1 §6.2 performance-contract benches — the Verter-vs-TS/tsgo benchmark fixtures on the same semantic queries + the per-family fallback-bound benches — perf-regression-gated as part of TERMINAL ACCEPTANCE (a family's fallback-count bound exceeded, or a missing Verter-vs-tsgo fixture, fails the bench gate), not merely the functional gate.
- The final find-grep sweep removing every remaining legacy entry-point name, and the terminal-acceptance ledger assertions (all 362 `IgnoredTestRow`s `Lifted`; every `AdditionalProofRow` covered; no vacuous parent token).

Legacy deletions:
- Any remaining legacy entry-point names surfaced by the final find-grep sweep (the last legacy resolution / projection / adapter symbol names — removed, not renamed-around).
- Any LSP / MCP / playground / schema-helper path that re-resolves a type instead of consuming the published `TypeInfoGraphPayload` / `FrameworkSurfacePayload` (replaced by the thin structural consumer — the one-resolver rule; the integration layer is never a second resolver).
- Any bench corpus referencing a third-party checkout (`.integration-tests/repos/<third-party>/`) from a non-gated bench/test — replaced by the vendored corpora (Testing-Hermeticity; pinned by `external_corpus_paths_not_present_outside_gated_tests`).
- Any composite-surface-specific resolver / walker (folded into the shared U2 reducers + U6 solver + the U14 adapter).
- No projection-repair / second-engine path remains anywhere in the integration / composite / final-lift surface. Stated explicitly per the template.

SemanticQueryKey/facts touched: no NEW key; the composite surfaces dispatch the existing reducer + flow/call keys (`Instantiate`, `IndexedAccess`, `KeyOf`, `Conditional`, `MappedType`, `TemplateLiteralReduce`, `ProjectPath`, `ResolveCall`, `FlowReturn`) over the U2/U6 substrate and project through the U14 adapter; the integrations READ the published `TypeInfoGraphPayload`. Facts read: `Member` / `MemberPresence`, `RouteGeneration` / `ExportSurface`, `ModuleAugmentation`, `LibIntrinsic`, `TypeEnvOptions`, project-generation facts; the published payload's validity is the U10 result-DB `ReadSetSignature.facts` rail. Admission: inherits the dispatched keys' budgets (`KeyspaceBudget` / `CallResolutionBudget` / `FlowSliceBudget`); the integrations do no admission (they consume an already-admitted payload).

Exact test rows lifted (capability `CompositeSurfaces`, `menu_like.rs` / `message_list_like.rs` / `table_like.rs`):
- menu_like.rs::menu_like_model_value_resolves_nested_conditional_utilities
- menu_like.rs::menu_like_slot_payload_extracts_item_and_model_value
- message_list_like.rs::message_list_like_extracts_pick_from_inferred_array_element
- message_list_like.rs::message_list_like_slot_remaps_payload_with_message_context
- table_like.rs::table_like_dynamic_slot_projection_uses_template_literal_keys

(These five `CompositeSurfaces` rows are the ONLY `IgnoredTestRow`s this block lifts. The `MacroResolution` row is owned by U14; the nine `jsx.rs` `JsxResolution` rows are owned by `U2.JSX_FOUNDATIONS`; every other substrate's rows are owned by their respective U-block. U15's lift over the FULL manifest is the terminal-acceptance assertion that every other block has already lifted its rows — see "Exact test rows lifted (terminal)" below — not a re-claim of those rows. The hand-authored parent §10.4.1 row→block_id partition assigns each row to exactly one `block_id` (the §10.4 generated coverage table — the U13/U15-gated unbuilt residual — is checked against it when it lands).)

Exact test rows lifted (terminal — the whole-manifest acceptance, NOT a re-claim):
- The terminal-acceptance assertion over the live ledger asserts every one of the 362 `IgnoredTestRow`s carries `status == Lifted` (no `Ignored`) once every owning block has landed (its `Typeinfo-Block:` trailer merged) — `basic.rs` (U14) + every U2 / U6 / U3 / U10 / U11 substrate row + the five `CompositeSurfaces` rows (this block). The ONLY source `#[ignore]`s permitted to remain are the Svelte/React STOP-gate files (`svelte_adapter_stop_gate.rs` / `react_adapter_stop_gate.rs`), which are explicit out-of-scope gates and are NOT `IgnoredTestRow`s in the 362. This is the mechanical "done" of the whole parity effort, not an additional row claim.

Required new guards (PART 2 §§10.5, 11.7, 11.9, 12; the Capability Map → "the guarantee over the 362 rows"; Testing-Hermeticity):
- `all_typeinfo_parity_rows_lifted_except_stop_gates` — asserts every `IgnoredTestRow` carries `status == Lifted` and the only residual source `#[ignore]`s are the registered Svelte/React STOP-gate files (zero stale ignored parity rows). This is the terminal no-stale-ignored-rows assertion over all 362.
- `svelte_adapter_stop_gate_is_registered_out_of_scope` + `react_adapter_stop_gate_is_registered_out_of_scope` — assert each STOP-gate file is an explicit registered out-of-scope gate (not an `IgnoredTestRow`, not counted in `EXPECTED_TOTAL_IGNORED_COUNT` or the bijection).
- `bench_result_row_reports_cache_mode_sourcemap_batch_thread_hit_fallback` — asserts `BenchResultRow` reports cache mode / source-map policy / batch shape / thread count / hit count / fallback count (the typed bench schema; benchmarks report mode + policy + batch + threads + hits + fallbacks).
- `architecture_minimizes_fallback_entry_not_fallback_cost` — the PART 1 §6.2 governing-rule guard, landed HERE with the per-family fallback-bound benches: asserts the tracked + perf-regression-gated metric is each query family's fallback ENTRY count against its `BenchResultRow` bound (a family exceeding its bound fails the bench), and that the warm path is O(validate). The four perf-hardening guards baked into the engine sections — `flow_graph_build_is_shallow_interned_no_lowering_lazy_regions` (U6.FLOW_RETURN_SUBSTRATE), `cache_key_axes_are_minimal_and_normalized` (U2.QUERY_VALUE_DOMAIN / U3.CACHE_FACT_MODEL), `relation_negative_and_unknown_paths_are_fast` (U2.RELATION_INFER) — are EXERCISED by these benches at terminal acceptance (a regression in any surfaces as a fallback-count / build-cost / negative-path-cost bench regression here), not re-owned.
- The Verter-vs-TS/tsgo fixtures + the per-family fallback-bound benches (PART 1 §6.2) are part of the perf-regression-gated terminal acceptance: a missing Verter-vs-tsgo fixture on the in-scope semantic queries, or a family's fallback count over its bound, fails the bench gate.
- The carried-forward terminal guards this block keeps green at acceptance: `ignored_test_row_table_holds_exactly_362_rows`, the source-`#[ignore]` ↔ `Ignored` ↔ `EXPECTED_TOTAL_IGNORED_COUNT` bijection/count guards, `no_landed_typeinfo_block_has_live_ignored_rows`, `no_vacuous_parent_u_block_landing`, `every_manifest_row_has_non_placeholder_mechanism_and_executable_proof`, `capability_rows_map_to_expected_query_fact_mechanisms`, and `external_corpus_paths_not_present_outside_gated_tests`.

Critical-rule guards: this block implements the parent's `(CRITICAL)` one-resolver rule (the integration / composite / final-lift surface routes through the one engine; the integration layer is never a second resolver), the Component-Meta Native vs Compat Rule (the composite surfaces project through the U14 thin adapter), and the Testing-Hermeticity rule (vendored bench corpora only). The STOP-gate + bench-schema + hermeticity + terminal-acceptance guards above are these rules' R6 guards. If this block lands any new `(CRITICAL)` STOP-gate / terminal-acceptance rule text in docs, it registers the corresponding guard here in the same change.

Proof requirement: per-row — the five `CompositeSurfaces` rows are `OracleAndGuard` (a TS7 oracle pin on each resolved end-to-end surface shape paired with a structural assertion that the surface resolves through the shared reductions + the U14 adapter without a composite-specific resolver). Consumed by each row's §10.3 proof-consumption rail (PART 2 §10.3; landed shape: the registry-bound driver-calling row body). The terminal-acceptance assertions (`all_typeinfo_parity_rows_lifted_except_stop_gates`, the STOP-gate guards, the bench-schema guard, the hermeticity guard) are `StructuralGuard`-class default-suite tests; the whole-manifest count + bijection + coverage guards are the carried-forward `StructuralGuard`-class guards over the live ledger.

Exit acceptance:
- All five `CompositeSurfaces` rows lift and pass on the normal `lib*.d.ts` corpus; each composite surface resolves end-to-end through the shared U2 reducers + U6 solver + the U14 adapter (no composite-specific resolver).
- The host integrations are live: LSP hover → graph+display + completion → framework surface, the MCP `typeinfo.*` / `component-meta.*` tools, the Zod/schema helpers, and the playground type explorer — each a thin structural consumer.
- The Svelte/React STOP-gate files are registered out-of-scope; the typed bench schema (`BenchResultRow` + vendored cm corpus benches + `MAX_TEST_TIMEOUT`) is in place over vendored corpora; the final find-grep sweep removed every remaining legacy entry-point name.
- The PART 1 §6.2 performance-contract benches are in place and green at the family bounds: the Verter-vs-TS/tsgo fixtures run over the same semantic queries (reported via `BenchResultRow`), and the per-family fallback-bound benches hold every family's fallback count under bound — perf-regression-gated as part of terminal acceptance (`architecture_minimizes_fallback_entry_not_fallback_cost`).
- **Terminal acceptance (the whole parity effort):** see the dedicated terminal-acceptance section below — all 362 `IgnoredTestRow`s `Lifted` (zero stale ignored except the STOP-gates), every `AdditionalProofRow` covered + non-placeholder, no vacuous parent token, and a green full workspace gate over the exact accepted content.

Verification commands:
- `cargo test --package verter_session` composite-surface / integration tests (`menu_like` / `message_list_like` / `table_like`) and the STOP-gate guards.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (the terminal-acceptance assertions: count 362, all `Lifted` except STOP-gates, bijection, coverage, no vacuous parent).
- `pnpm vitest --run` the `@verter/component-meta` integration / schema specs and the LSP / MCP integration specs; the VS Code E2E suite for hover/completion (`/e2e-vscode-testing`).
- `pnpm run build:native && pnpm run build:lsp:release && pnpm run build:mcp:release` for the bench/integration release builds (benchmarks only); the vendored cm corpus benches (`component_meta_cold` / `_warm`); the PART 1 §6.2 performance-contract benches (the Verter-vs-TS/tsgo fixtures + the per-family fallback-bound benches — perf-regression-gated, family fallback counts under bound).
- The five lifted-row proofs via the generated wrapper (or `cargo test … -- --ignored` before the branch strips the `#[ignore]`s).
- The full workspace gate (the CI gate — the complete Rust **AND** JavaScript gate, green only when BOTH pass; PART 2 §11.2): `cargo nextest run --workspace` + `cargo test -p verter_session --tests` (the canonical Rust pair — bare `cargo test --workspace --tests` silently skips the verter_session integration suite and is NOT the gate); `cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`; `pnpm test`; `pnpm install --frozen-lockfile`.
- `node scripts/gen-corpus-audit-tests.mjs` (idempotent; audit fixtures change with the integrations).
- Commit cadence / review gate: PARENT-UNIFORM — the uniform discipline for EVERY block in this subplan (parent PART 2 §11.11 / §11.12), stated once and not restated per block: each block (including this terminal block) lands as ONE squashed commit (WIP series during the work, no per-commit gate) after the three-reviewer LAND verdict (1 Claude Code + 2 codex).

Docs updated: keep the `/component-meta` (framework-adapter registry), `/architecture` (MCP/LSP/playground integration), `/build-and-profiling` (bench schema), `/testing` (hermeticity), and `/e2e-vscode-testing` (hover/completion) sections current; record the final-state manifest counts (all parity rows `Lifted`, the STOP-gate residuals) in the `/testing` skill's unignore-manifest notes.

Re-entry notes: idempotent. If partial, the manifest tells exactly which `CompositeSurfaces` rows still carry `#[ignore]` and whether any other block's rows remain `Ignored` — i.e. whose `Typeinfo-Block:` trailer is unmerged (the terminal-acceptance assertion fails until every owning block has landed). U15 is terminal — do NOT land its parent token while any child block's rows remain `Ignored` (`no_vacuous_parent_u_block_landing`). The integration layer is a thin consumer — if an integration path re-resolves a type, route it through the published payload instead. Do not vendor a third-party corpus into a non-gated bench.

---

## Terminal acceptance — the mechanical "done" for the whole parity effort

U15 is where the unified §9 terminal acceptance is satisfied for the native-parity
scope. "Done" for the whole effort is NOT a prose claim — it is the conjunction of
the following mechanical checks over the live ledger + the merged `Typeinfo-Block:`
trailers + the full CI gate (PART 2 §§10.5, 11.5, 11.7, 11.9, 12), each pinned by a
named guard. The parity effort is complete iff ALL hold:

1. **Every `IgnoredTestRow` is `Lifted`.** The `IgnoredTestRow` table holds EXACTLY
   362 rows (`ignored_test_row_table_holds_exactly_362_rows`), and every one carries
   `status == Lifted` — zero `Ignored`
   (`all_typeinfo_parity_rows_lifted_except_stop_gates`). Concretely, every owning
   block has landed its row-set (its `Typeinfo-Block:` trailer merged): `U2.RELATION_INFER` / `U2.UTILITIES` /
   `U2.INDEXED_ACCESS` / `U2.MAPPED_TEMPLATE` / `U2.CLASS_SURFACES` / `U2.ENUMS` /
   `U2.MODULE_AUGMENTATION` / `U2.JSX_FOUNDATIONS` (the U2 reducer + JSX rows);
   U6 (the flow / call / contextual / value-inference rows); U3 (the `cross_file.rs`
   route-demand rows); U10 (the `mode_boundary_invariants.rs` /
   `expansion_boundaries.rs` / `demand_boundary.rs` projection-exactness rows);
   U11 (the `footprint.rs` / `cache_invalidation.rs` / `demand_boundary.rs`
   footprint rows); U14 (the single `basic.rs` `MacroResolution` row); and U15 (the
   five `CompositeSurfaces` rows). No parity row is left ignored.

2. **The only residual `#[ignore]`s are the explicit STOP-gates.** The
   source-`#[ignore]` ↔ `Ignored`-rows ↔ `EXPECTED_TOTAL_IGNORED_COUNT` bijection
   holds with `EXPECTED_TOTAL_IGNORED_COUNT == 0` over the parity rows; the only
   source `#[ignore]`s that remain are the registered Svelte/React STOP-gate files
   (`svelte_adapter_stop_gate.rs` / `react_adapter_stop_gate.rs`), which are
   explicit out-of-scope gates, are NOT `IgnoredTestRow`s, and never enter the count
   or bijection (`svelte_adapter_stop_gate_is_registered_out_of_scope`,
   `react_adapter_stop_gate_is_registered_out_of_scope`).

3. **Every `AdditionalProofRow` is covered and non-placeholder.** Every
   `AdditionalProofRow` (the closed set of exactly 7 coverage-only rows = the 6
   JSX no-new-key submatrix rows owned by `U2.JSX_FOUNDATIONS` + the 1 mapped
   companion `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property`
   owned by `U2.MAPPED_TEMPLATE`) resolves to a
   non-placeholder `mechanism_id` and an executable `ProofRequirement`, and maps to
   the query/fact mechanisms its capability is supposed to use
   (`every_manifest_row_has_non_placeholder_mechanism_and_executable_proof` +
   `capability_rows_map_to_expected_query_fact_mechanisms`, both spanning BOTH
   tables). The coverage table is complete and non-placeholder over every manifest
   row in both tables.

4. **No parent / aggregate U-block token is vacuously landed.** Every parent
   U-block token (`U2` … `U15`) is the aggregate over its child blocks' UNION
   row-set and is done only when every row in that union is `Lifted` — never by
   owning zero rows (`no_vacuous_parent_u_block_landing`); no landed block has a live
   `Ignored` row (`no_landed_typeinfo_block_has_live_ignored_rows`); the merged
   `Typeinfo-Block:` trailers on the target branch agree with the manifest status
   (each merged block's rows are `Lifted`; each block with `Lifted` rows has a merged
   trailer — `typeinfo_block_lands_as_single_squashed_commit`, PART 2 §11.11).

5. **The full CI gate is green over the merged content.** Each block — including
   U15 — reached `Lifted` + a merged `Typeinfo-Block:` trailer only through the
   git/CI landing protocol: green CI (PART 2 §11.2) AND the three-reviewer LAND
   verdict (1 Claude Code + 2 codex; PART 2 §11.12), its WIP series squash-merged to
   ONE target-branch commit (PART 2 §§11.4, 11.11). The CI gate is the complete Rust
   **AND** JavaScript gate, green only when BOTH pass: `cargo nextest run --workspace` + `cargo test -p verter_session --tests`
   (the canonical Rust pair — bare `cargo test --workspace --tests` silently skips the
   verter_session integration suite and is NOT the gate),
   `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`, `pnpm test`,
   and `pnpm install --frozen-lockfile` all green, with the bench corpora vendored
   (`external_corpus_paths_not_present_outside_gated_tests`) and the typed bench
   schema reporting cache mode / source-map policy / batch shape / thread count /
   hit count / fallback count
   (`bench_result_row_reports_cache_mode_sourcemap_batch_thread_hit_fallback`).

This is the composition the Capability Map names as "the guarantee over the 362
rows": the two-table ledger with the exact-362 count + bijection (PART 2 §10.5);
the U0 row-exact capability→mechanism→proof coverage table that DEFINES completeness
(PART 2 §10.4 — the U13/U15-gated residual); the per-row executable `ProofRequirement`
with the proof registry + row-test rail (PART 2 §§10.2–10.3 — landed under U0-FINISH-B
in the locked design's hand-authored shape); the git/CI landing protocol (PART 2
§11); the no-skip guarantee (PART 2 §12); and the git/manifest-driven, parallel-safe
resume protocol (PART 2 §14). When all five checks hold, the 362-row
parity is mechanically tracked from `Ignored` to `Lifted`, never skipped and never
vacuously satisfied — and the native typeinfo engine is the full TypeScript-parity
checker the parent architecture specifies, with the LSP / MCP / component-meta /
playground consumers reading the one published surface.
