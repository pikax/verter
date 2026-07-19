# Globally-Registered Component Typing on the IDE Surface

How Verter types template tags for components registered ONLY through the
`GlobalComponents` interface (a `components.d.ts` / UI-kit augmentation, an
`app.component()` call surfaced by an augmentation) — never imported by the
SFC — and how that machinery stays fail-open for web components.

## Mechanism

Per script arm (`<script setup>`, Options API, no-script), IDE codegen emits one
**fallback const** per unresolved component tag
(`crates/verter_compiler/src/ide/script/wrapper.rs`), and the template arm
rewrites dashed tags onto those consts
(`crates/verter_compiler/src/ide/template/mod.rs`). The authoring form selects
the const's type arm:

| Authoring | Const type | Unregistered outcome |
| --- | --- | --- |
| Pascal anywhere (`<GlobalCountComp>`) | `GlobalComponentType<'Name'>` | fail-CLOSED `unknown` → real TS2604 at the tag |
| kebab/lowercase only (`<global-count-comp>`, `<ion-button>`) | `GlobalComponentKebabType<'Name', 'authored-tag'>` | fail-OPEN: function component over `JSX.IntrinsicElements['authored-tag']` (user web-component augmentations keep typing; Vue's `[name: string]: any` index otherwise yields `any`) — never a false TS2604 |
| configured `custom_elements` prefix match | none (excluded from collection AND rewrite, even over a same-name local binding) | tag stays authored — plain intrinsic |

Registered names resolve identically in both arms (Pascal key first, then the
authored key), so a kebab-only file keeps full prop/event/definition typing.
Rationale: compile-time cannot know `GlobalComponents` membership, and
resolver-derived inventories must not enter the parse-domain compile layer
(env-hash split), so registered-vs-unregistered discrimination lives in the
type system — where the knowledge exists and invalidation is free.

The kebab rewrite maps **per segment** (`emit_mapped_kebab_pascal_rewrite`):
separators are deleted, only case-changing segment heads are overwritten,
every other byte stays an `Original` chunk — so every LETTER column of the
authored tag (including the last) maps through the `PositionMapper`; only the
removed `-` columns stay unmapped (fail-closed).

The conditional types + the empty `declare module "vue" { interface
GlobalComponents {} }` augmentation (introduce-on-absence for Vue versions
whose types export no `GlobalComponents`; merge with user augmentations
otherwise) ship in five synchronized copies — see the `/compiler-codegen`
skill's "GlobalComponents Fallback Consts" section for the list and the guard
tests.

## Verified compatibility facts (recorded 2026-07-19, vue 3.5.40 on-disk)

- `@vue/runtime-core` DECLARES `export interface GlobalComponents` and its own
  docs direct users to augment `@vue/runtime-core`; `@vue/runtime-dom` extends
  it; `vue` re-exports it (`export * from '@vue/runtime-dom'`). A project
  augmentation of `@vue/runtime-core` therefore merges into the SAME interface
  identity that `import("vue").GlobalComponents` resolves — on every layout
  where `vue` re-exports the interface, legacy runtime-core augmentations are
  visible to Verter's conditionals.
- The ≤3.4-shaped case (no `GlobalComponents` export on `vue` at all) is
  covered by the committed discriminator legs in
  `packages/typescript-plugin/src/helpers/verterTypesStub.spec.ts` (the empty
  augmentation introduces the surface; the discrimination control proves the
  legs fail without it).

## Deferred debt rows

| ID | Deferral | Owner block | Resolution gate | Acceptance test |
| --- | --- | --- | --- | --- |
| GC-D1 | **Shared-tsgo real-provider leg.** `real_provider_test!` generates tsserver + managed-tsgo variants only; the serving-topology `shared-tsgo` (attach to the editor's tsgo) has no headless-harness leg, so global-component acceptance on that topology is asserted only transitively (same server code path, same generated carrier). | LSP test harness (`crates/verter_lsp/src/test_harness.rs`) — the shared-provider harness extension that `shared_provider_live.rs` seeds | When a shared-topology harness lands (the attach-to-editor lane), add the third `real_provider_test!` variant and run the global-component suite on it | The same 4 tests × shared leg, receipts under `VERTER_REQUIRE_*` |
| GC-D2 | **Legacy module-key union.** A project whose `vue` types do NOT re-export `GlobalComponents` (pre-re-export layouts) and that augments only `@vue/runtime-core`/`@vue/runtime-dom` registers into an interface Verter's `import("vue")`-keyed conditionals cannot see: Pascal-authored tags of such components fail closed (TS2604); kebab-authored tags stay silently `any` (fail-open, pre-existing behavior). The Volar-style multi-module union needs a presence guard — a bare `import("@vue/runtime-core")` reference in the shipped d.ts errors on layouts without the package (pnpm strict) and would poison the common case. | `@verter/types` conditionals (all five copies) | Union lands only with a guarded-resolution design (per-module presence probe or generated per-project types), validated on pnpm-strict + npm-hoisted layouts | A spec leg with a vue stub that does NOT re-export the interface plus a runtime-core augmentation, asserting registered members type |
| GC-D3 | **`GlobalDirectives` on pre-3.5 Vue.** `@verter/types` references `import("vue").GlobalDirectives` (directive typing) without an introduce-on-absence augmentation; on a Vue whose types lack the export, the reference errors inside the shipped d.ts (suppressed under `skipLibCheck`, degrading directive typing to `any`). Same mechanism as the shipped `GlobalComponents` augmentation — not yet applied. | `@verter/types` (all five copies) | Next `@verter/types` surface change | Extend the ≤3.4 spec leg's vue stub to drop `GlobalDirectives` and assert the contract still checks |

## Residual known deltas (disclosed, non-blocking)

- An unregistered, unconfigured dashed tag is REWRITTEN (to the fail-open
  kebab const), so its hover shows the const rather than the bare intrinsic
  property, and go-to-definition is fail-closed EMPTY (pre-fallback it could
  land on Vue's `IntrinsicElements` index-signature declaration). Attribute
  typing and diagnostics match the pre-fallback intrinsic behavior.
- The removed `-` separator columns of a rewritten kebab tag are unmapped
  (1 column per dash); every letter column maps.
- Hover text for fallback consts renders the conditional type ALIAS
  (`GlobalComponentType<…>` / `GlobalComponentKebabType<…>`) when the provider
  does not expand it; the reserved `___VERTER___` prefix itself is stripped in
  the display layer (`type_provider/merge/hover.rs::strip_synthetic_prefix`).
