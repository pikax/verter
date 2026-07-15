# Verter component-meta semantic-correctness audit — nuxt-ui corpus

Date: 2026-07-15. Worktree: `/Users/carlosrodrigues/Documents/dev/verter-codexport` (branch `perf/codexraw-port`, release build).
Corpus: `/Users/carlosrodrigues/Documents/dev/verter/.integration-tests/repos/nuxt-ui` (179 `.vue` components).
Method: `bench:meta:ui` sweep (`--backends=verter --scenarios=repo_first_pass --expected=vue-component-meta`, artifacts in `/tmp/correctness-sweep/`), then an in-process per-component re-diff (script reproduced the bench totals EXACTLY), then per-field adjudication **from component source + imported declarations** (vue-component-meta = cross-reference only, not ground truth).

## 1. Sweep result

- Outcomes: **179/179 success** (0 degraded / 0 query_error / 0 crash). Whole verter repo pass ≈ 19 s (vcm expected build: ~2.5–10 s **per component**).
- deviationTotals vs vcm: `exactMatches=3, totalMissing=43, totalExtra=357, totalFieldMismatches=3268` (176/179 components deviate somewhere).
- Per-component ranked summary: `/tmp/correctness-sweep/per-component-summary.json`; per-component verter artifacts + diffs: `/tmp/correctness-sweep/per-component/*.{verter,diff}.json`.

## 2. Deviation decomposition (every deviation classified)

| Family | Count | Adjudication |
|---|---|---|
| verter-extra `class`/`style` props | 318 | **Harness artifact.** Source declares them (`class?: any` etc.); the bench refiner drops intrinsics on the vcm side because only verter carries the `declared_in_macro_type_arg` sidecar fact (`refineMetaForBenchmark` falls back to `false` for vcm). Verter matches source. |
| verter-extra `exposed` members (36) + `onSubmit` shadow (4) | 39 | **Verter right / vcm bug** (see §4.1) + refiner shadow-event artifact for `onSubmit`. |
| Missing (verter lacks) | 43 | **3 real verter bugs** (§3.1–3.3) + 1 knock-on (§3.5c). |
| Union arm order (top-level) | 122 | Benign — same set, different order. |
| `X` vs `X \| undefined` width | 44 | Benign-ish (`?:`-implied undefined printed by one side only). |
| Shallow carrier vs expansion (type text) | 701 | **Policy, by design** (Shallow-By-Default): verter keeps source-faithful carriers (`Badge["slots"]`, `ApplyModifiers<T, Mod>`, `CoreOptions<T>["state"]`); vcm expands. Content equivalent. |
| Schema depth (empty shallow schema) | 175 + 514 | Same policy at the `schema` field. |
| propsJsonSchema mirror | 596 | Double-counts prop diffs — no independent signal. |
| Slot binding types degraded to name-carriers | 340 | **Verter fidelity gap** (§3.6): `{ open: open; }` instead of `{ open: boolean; }`. |
| tags | 138 | 124 = **vcm loses tags** (verter superset, matches source JSDoc, e.g. reka `@defaultValue`); 9 verter loses; 5 = whitespace in multiline tag text. |
| description | 79 | 57 = **vcm loses event descriptions** (reka emit JSDoc; verter right); 15 = **verter loses prop descriptions** (§3.7); 7 both-differ. |
| default | 17 | Benign: quote style (`'value'` vs `"value"`); 1 = factory display `() => ({deep: true})` vs unwrapped `{deep: true}` (Table.watchOptions — vcm unwraps, verter shows source expression). |
| Residual type/schema texts | 542 | Mix of: benign formatting (`{ base?: any }` vs `{ base?: any; }`, `Array<T>` vs `T[]`, default-type-arg display, generic-arg order); verter printer defects (§3.5); vcm junk (§4.3); the two semantic bugs (§3.3, §3.4). |

**Corpus-wide invariants that held:** zero `required` mismatches; zero event-name mismatches; prop/slot name sets complete except §3.1–3.3; defaults agree modulo quote style.

## 3. Confirmed VERTER bugs (source-adjudicated)

### 3.1 Pick over an instantiated generic heritage drops ALL members — ContentSearch (severity: HIGH, biggest real gap)
`ContentSearch.vue:58`: `interface ContentSearchProps<T…> extends Pick<ModalProps, 'title'|…9 keys>, Pick<CommandPaletteProps<CommandPaletteGroup<ContentSearchItem>, ContentSearchItem>, 'icon'|…18 keys>`.
Verter resolves the `Pick<ModalProps,…>` heritage (all 9 present) but contributes **zero** members from the `Pick<CommandPaletteProps<…instantiated generic args…>, …>` heritage: 18 props missing (`icon, trailingIcon, selectedIcon, childrenIcon, placeholder, autofocus, loading, loadingIcon, closeIcon, back, backIcon, disabled, highlightOnHover, labelKey, descriptionKey, preserveGroupOrder, virtualize, groups`). vcm has all 18; source declares them. Discriminator: Pick over a bare interface works (TooltipContent etc.); Pick whose SOURCE is a generic instantiation with nested type args fails.

### 3.2 Pick over a generic slots type drops generic-value-dependent keys — DropdownMenuContent (severity: HIGH)
`DropdownMenuContent.vue:45`: `type DropdownMenuContentSlots<A,T> = Pick<DropdownMenuSlots<A>, 'item'|'item-leading'|'item-label'|'item-description'|'item-trailing'|'empty'|'content-top'|'content-bottom'> & {default} & DynamicSlots<…>`.
Verter publishes only `empty, content-top, content-bottom, default` — the 5 picked keys whose value types reference the generic (`SlotProps<T>` / props mentioning `T`) are DROPPED instead of published shallow. Sharp boundary: **DropdownMenu.vue itself (direct `defineSlots<DropdownMenuSlots<T>>()`) publishes all 9 slots correctly on both backends** — only the Pick path loses them. Source truth: all 5 are static members of `DropdownMenuSlots` (DropdownMenu.vue:123-138).

### 3.3 Distributive conditional over a defaulted union type-arg evaluated non-distributively — Accordion emit (severity: MEDIUM, semantic wrong-answer)
`reka-ui/dist/index4.d.ts:248`: `AccordionRootEmits<T extends SingleOrMultipleType = SingleOrMultipleType> = { 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }`; nuxt-ui `AccordionEmits extends AccordionRootEmits {}` (default arg `'single'|'multiple'`).
TS truth (distributive): `string | string[] | undefined` (= vcm). Verter: `string[] | undefined` — only the false branch; the `string` payload arm is lost. Violates the codebase's own "closed conditionals reduce immediately" rule — reduction must distribute over naked-type-param unions.

### 3.4 COMPAT_BLOCKED_SLOT_NAMES over-blocks a real declared slot — Popover `anchor` (severity: MEDIUM)
`Popover.vue:60` declares `anchor?(props: SlotProps<M>): VNode[]`. Verter's compat projection suppresses it (`anchor` ∈ `COMPAT_BLOCKED_SLOT_NAMES`, `packages/component-meta/src/published-surface.ts`), but vcm itself PUBLISHES the slot — the blocklist (meant to mirror vcm's VNode-transport suppression) over-approximates for at least `anchor`. Any userland slot named `placeholder`, `target`, `el`, `component`, … would be silently hidden the same way.

### 3.5 Type-text printer defects (severity: MEDIUM for display consumers; underlying IR mostly correct)
a) **Missing precedence parens around function types in unions**: `(originalRow: T, index: number) => string | undefined` (Table.getRowId; parses wrong) vs correct `((…) => string) | undefined`; same on onHover/onSelect/mergeOptions, Editor.onBeforeCreate `(props: EditorEvents["beforeCreate"]) => void | undefined`, EditorMentionMenu.appendTo, SelectMenu.by.
b) **Duplicated arms**: ColorModeSelect.valueKey `undefined | undefined`; Calendar.prevPage/nextPage `(placeholder: DateValue) => DateValue & (placeholder: DateValue) => DateValue | undefined` (same fn twice via `&`); EditorToolbar.appendTo union repeats both arms twice.
c) Knock-on: the `undefined | undefined` text also knocks `valueKey` out of `propsJsonSchema` (the converter bails) — the 1 remaining "missing" row.
d) **`Array<union>` mis-print with real semantic misread**: reka `collisionBoundary?: Element | null | Array<Element | null>` (index4.d.ts:1213) → verter `Element | null | Element | null[] | undefined` (dup `Element`, and `(Element|null)[]` became `null[]`) — as written this is a DIFFERENT type; vcm prints it correctly.
e) **Raw source-slice leakage**: SelectMenu.virtualize / highlight-event payload type texts contain verbatim source newlines + JSDoc comments (`boolean | {\n /**\n * Number of items…`). Type text should be canonical.
f) `(string & {})` collapsed to `string` (Link/Button rel/target) — assignability-equivalent, loses the literal-autocomplete idiom.

### 3.6 Slot binding value types degrade to synthetic name-carriers (severity: MEDIUM, systematic — ~340 rows)
`{ open: open; }`, `{ item: item; index: index; ui: ui; }` etc. (Tooltip/Accordion/Badge/… slots) where source + vcm say `{ open: boolean }`, `{ item: T; index: number; … }`. The `syntheticSlotBinding` carrier surfaces the binding NAME as the member type. Some bindings DO resolve (Calendar heading: `view: "day"|"month"|"year"`, `date: DateValue`), so the machinery exists; most ui/generic-parameterized bindings don't.

### 3.7 Narrow JSDoc description loss on props (severity: LOW — 15 rows)
13 on EditorToolbar (props declared in a type-ALIAS object literal composed via intersection/conditional — `EditorToolbarBaseProps`; the `@defaultValue` TAG survives, the leading prose description is dropped), plus Link.custom (doc on `RouterLinkProps.custom` in vue-router dts), Table.meta (base-declaration doc fallback). vcm keeps them.

### 3.8 defineModel emit payload width (severity: LOW)
Emits generated by `defineModel` (Table's 13 `update:*`, part of Accordion §3.3) publish payload `T` where TS/vcm derive `T | undefined` (optional model). Minor narrowing.

Also noted (not a bug, policy inconsistency): depth is provenance-dependent — Separator.avatar fully inlined while Badge.avatar stays an `AvatarProps` carrier; Theme.props expands `ThemeDefaults` into the ~100-key map while `ui: ThemeUI` stays a carrier; Button color resolved to literals but activeColor left as carrier. All content-correct, but "shallow-by-default" is not applied uniformly.

## 4. Confirmed VCM bugs (source-adjudicated; verter is right)

1. **`exposed` is empty corpus-wide.** Sources use `defineExpose` in 20+ components (Input.vue:182 `inputRef`; Table.vue:518 `$el`/`tableRef`/`tableApi`; SelectMenu.vue:511 `triggerRef`/`viewportRef`; Stepper `next/prev/hasNext/hasPrev`; AuthForm `formRef`/`state`; Carousel, Editor, FileUpload, PinInput, Tabs, Textarea, Tree, ScrollArea, ContentSearch…). vcm publishes none of them; verter publishes all 36 (types are shallow `ReturnType<typeof useTemplateRef>` carriers; Table `$el` shows `unknown` vs source `HTMLElement` — minor).
2. **Event descriptions dropped** on 57 rows (reka-ui emit JSDoc, e.g. Accordion `update:modelValue` "Event handler called when the expanded state of an item changes"); verter preserves them.
3. **Tags dropped** on 124 rows (`@defaultValue`, `@see` from node_modules dts props — Stepper.linear, Carousel.fade, Accordion.type, withDefaults-derived); verter preserves/synthesizes them (verter also synthesizes `@defaultValue` tags from `withDefaults` — an enrichment vcm lacks).
4. **Junk expansions**: `keyof Extract<T, object>` schemas explode into String.prototype dumps (`__@iterator@516`…) on Accordion/others' labelKey/valueKey; EditorToolbar.appendTo collapses to `unknown`; internal rolled-up alias names leak (`it`/`et` — verter leaks different ones, `St`/`vt`: parity, both ugly).

## 5. Control set + zero-deviation sanity (adjudicated from source)

| Component | Surface completeness vs source | Verdict on remaining diffs |
|---|---|---|
| Avatar (dev 12) | props/slots complete (incl. `@vue-ignore` ImgHTMLAttributes members — BOTH backends type-level-include them; `class`/`style` verter-only = harness) | union order, ui/chip carriers, slot `{} \| undefined` width — all benign/policy. `icon: any` CORRECT on both (`IconProps['name']` = `string \| any` ⇒ `any`). |
| Badge (14) | complete | carriers + slot-opaque `{ ui: ui }` (§3.6). |
| Button (39) | complete incl. vue-router members (`to/replace/ariaCurrentValue/viewTransition/exactActiveClass`) — enrichment CONFIRMED, vcm agrees | carriers, union order, `(string & {})` collapse (§3.5f), minified alias parity. |
| Link (26) | complete | ditto + `custom` description loss (§3.7); synthesized `@defaultValue undefined` tags (enrichment). |
| Input (36) | complete; exposed `inputRef` verter-only (vcm bug §4.1) | carriers (`ApplyModifiers<T, Mod>` vs vcm's leaked `_Number<_Optional<…>>` internals — verter's display is arguably better), slot-opaque. |
| SelectMenu (81) | complete; exposed verter-only | carriers; source-slice leakage §3.5e; `M & boolean` source-exact (vcm simplified). |
| Table (96) | complete (whole TanStack surface, incl. `declare module '@tanstack/table-core'` augmentation-dependent meta/class/style columns); exposed verter-only | fn-parens §3.5a; defineModel `\| undefined` §3.8; watchOptions factory default display; carriers; `Row` missing `<T>` in expanded slot. |
| Accordion (28) | complete | **§3.3 emit payload bug**; slot-opaque; carriers; vcm keyof junk (§4.4). |
| Separator (10) | complete | avatar inlined (policy inconsistency), carriers. |
| Theme (4) | complete | ThemeDefaults expansion vs name (both correct). |
| **Zero-dev**: OverlayProvider, prose/CodeIcon, prose/Script | verified against source: OverlayProvider has NO macros ⇒ all-empty correct; CodeIcon `{icon?: any, filename?: string\|undefined}` correct incl. any-absorption; Script `{src: string, required: true}` correct | agreement is genuine, not shared-wrong. |
| Near-zero: prose/Strong (and prose family) | complete | only `{ base?: any }` vs `{ base?: any; }` formatting. |

Enriched-12 check: Link/Button (above), AuthForm (props parity incl. `onSubmit`; `submit` emit matches; exposed verter-only), Pagination (FULL parity except harness `class`; all reka Pick members + defaults agree), ContentSearch (**§3.1 bug — the one enriched component with a real regression surface**). vcm agrees with the enriched vue-router values everywhere it can (as predicted).

## 6. Verdict

- **Structural surface extraction (names, presence, required, defaults): HIGH confidence.** 0 required mismatches, 0 event-name mismatches, complete prop/slot inventories on 176/179 components; the exceptions are exactly ContentSearch (18 props), DropdownMenuContent (5 slots), Popover (1 slot).
- **Semantic type CONTENT: MOSTLY correct** with two real evaluation bugs (Pick-over-instantiated-generic §3.1/§3.2 — probably one root cause; distributive conditional §3.3) and one policy over-block (§3.4).
- **Type TEXT rendering: the weakest layer** — parens/dup-arms/`Array<union>`/source-slice defects (§3.5) misrender otherwise-correct IR; worth a dedicated printer-hardening pass since these texts are consumer-visible.
- **vs vue-component-meta overall**: verter is RICHER on exposed/tags/event-docs (vcm demonstrably buggy there), EQUAL on core surfaces, ~9× faster on this corpus, and behind only on the four named bug clusters + display fidelity of slot bindings/type texts.

Raw evidence: `/tmp/correctness-sweep/` (run JSON, expected artifacts, per-component verter artifacts + diffs, `f-semantic-items.json`, `f-residual.json`, `per-component-summary.json`); comparison log `/tmp/compare-run-log.txt`; sweep log `/tmp/correctness-sweep-log.txt`.
