/**
 * Declarative matrix cases for dense IDE coverage.
 * Each case becomes a required hard-fail test ID.
 */
import type { TokenAnchor } from "./parityHarness";

export type MatrixKind =
  | "clean"
  | "hover"
  | "definition"
  | "definition-file"
  | "completion"
  | "references"
  | "hover-range"
  | "no-virtual-definition";

export interface MatrixCase {
  readonly id: string;
  /** ISSUES.md row — required for ledger completeness even when the test hard-fails. */
  readonly issue: string;
  readonly kind: MatrixKind;
  readonly file: string;
  readonly anchor?: TokenAnchor;
  readonly target?: TokenAnchor;
  readonly targetFile?: string;
  readonly needles?: readonly string[];
  readonly completionLabels?: readonly string[];
  readonly minRefs?: number;
  readonly completionOffsetNeedle?: string;
  readonly completionOffsetExtra?: number;
}

export const VUE_MATRIX_CASES: readonly MatrixCase[] = [
  // Directives
  {
    id: "vue.matrix.directives.clean",
    issue: "ISSUE-vue-matrix-directives-clean",
    kind: "clean",
    file: "src/matrix/Directives.vue",
  },
  {
    id: "vue.matrix.directives.v-if.hover",
    issue: "ISSUE-vue-matrix-vif-hover",
    kind: "hover",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "show", occurrence: 2 },
    needles: ["show", "boolean"],
  },
  {
    id: "vue.matrix.directives.v-for.hover",
    issue: "ISSUE-vue-matrix-vfor-hover",
    kind: "hover",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "item", occurrence: 1 },
    needles: ["item"],
  },
  {
    id: "vue.matrix.directives.v-model.def",
    issue: "ISSUE-vue-matrix-vmodel-def",
    kind: "definition",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "model", occurrence: 1 },
    target: { file: "src/matrix/Directives.vue", token: "model", occurrence: 0 },
  },
  {
    id: "vue.matrix.directives.v-html.hover",
    issue: "ISSUE-vue-matrix-vhtml-hover",
    kind: "hover",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "raw", occurrence: 1 },
    needles: ["raw"],
  },
  {
    id: "vue.matrix.directives.v-bind.class.hover",
    issue: "ISSUE-vue-matrix-vbind-class",
    kind: "hover",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "klass", occurrence: 1 },
    needles: ["klass"],
  },
  {
    id: "vue.matrix.directives.v-on.def",
    issue: "ISSUE-vue-matrix-von-def",
    kind: "definition",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "onClick", occurrence: 1 },
    target: { file: "src/matrix/Directives.vue", token: "onClick", occurrence: 0 },
  },
  {
    id: "vue.matrix.directives.click-modifier.hover",
    issue: "ISSUE-vue-matrix-click-modifier",
    kind: "hover",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "prevent", occurrence: 0 },
    needles: ["prevent"],
  },
  {
    id: "vue.matrix.directives.computed.hover",
    issue: "ISSUE-vue-matrix-computed-hover",
    kind: "hover",
    file: "src/matrix/Directives.vue",
    anchor: { file: "src/matrix/Directives.vue", token: "label", occurrence: 2 },
    needles: ["label"],
  },
  {
    id: "vue.matrix.directives.completion-locals",
    issue: "ISSUE-vue-matrix-completion-locals",
    kind: "completion",
    file: "src/matrix/Directives.vue",
    completionOffsetNeedle: "{{ label }}",
    completionOffsetExtra: 3,
    completionLabels: ["label", "show", "items"],
  },

  // Slots / emits
  {
    id: "vue.matrix.slots.clean",
    issue: "ISSUE-vue-matrix-slots-clean",
    kind: "clean",
    file: "src/matrix/SlotsEmits.vue",
  },
  {
    id: "vue.matrix.slots.header-local.hover",
    issue: "ISSUE-vue-matrix-slot-header",
    kind: "hover",
    file: "src/matrix/SlotsEmits.vue",
    anchor: { file: "src/matrix/SlotsEmits.vue", token: "title", occurrence: 1 },
    needles: ["title", "string"],
  },
  {
    id: "vue.matrix.slots.default-local.hover",
    issue: "ISSUE-vue-matrix-slot-body",
    kind: "hover",
    file: "src/matrix/SlotsEmits.vue",
    anchor: { file: "src/matrix/SlotsEmits.vue", token: "body", occurrence: 1 },
    needles: ["body"],
  },
  {
    id: "vue.matrix.emits.pick.hover",
    issue: "ISSUE-vue-matrix-emit-pick",
    kind: "hover",
    file: "src/matrix/SlotsEmits.vue",
    anchor: { file: "src/matrix/SlotsEmits.vue", token: "pick", occurrence: 0 },
    needles: ["pick", "string"],
  },
  {
    id: "vue.matrix.emits.handler.def",
    issue: "ISSUE-vue-matrix-emit-handler",
    kind: "definition",
    file: "src/matrix/SlotsEmits.vue",
    anchor: { file: "src/matrix/SlotsEmits.vue", token: "onPick", occurrence: 1 },
    target: { file: "src/matrix/SlotsEmits.vue", token: "onPick", occurrence: 0 },
  },

  // Style v-bind
  {
    id: "vue.matrix.style-bind.clean",
    issue: "ISSUE-vue-matrix-style-bind-clean",
    kind: "clean",
    file: "src/matrix/StyleBind.vue",
  },
  {
    id: "vue.matrix.style-bind.accent.def",
    issue: "ISSUE-vue-matrix-style-bind-def",
    kind: "definition",
    file: "src/matrix/StyleBind.vue",
    anchor: { file: "src/matrix/StyleBind.vue", token: "accent", occurrence: 1 },
    target: { file: "src/matrix/StyleBind.vue", token: "accent", occurrence: 0 },
  },
  {
    id: "vue.matrix.style-bind.pad.hover",
    issue: "ISSUE-vue-matrix-style-bind-hover",
    kind: "hover",
    file: "src/matrix/StyleBind.vue",
    anchor: { file: "src/matrix/StyleBind.vue", token: "pad", occurrence: 1 },
    needles: ["pad"],
  },

  // JS surface
  {
    id: "vue.matrix.js.clean",
    issue: "ISSUE-vue-matrix-js-clean",
    kind: "clean",
    file: "src/matrix/JsSurface.vue",
  },
  {
    id: "vue.matrix.js.hover.count",
    issue: "ISSUE-vue-matrix-js-hover",
    kind: "hover",
    file: "src/matrix/JsSurface.vue",
    anchor: { file: "src/matrix/JsSurface.vue", token: "jsCount", occurrence: 2 },
    needles: ["jsCount"],
  },
  {
    id: "vue.matrix.js.def.user",
    issue: "ISSUE-vue-matrix-js-def",
    kind: "definition",
    file: "src/matrix/JsSurface.vue",
    anchor: { file: "src/matrix/JsSurface.vue", token: "jsUser", occurrence: 1 },
    target: { file: "src/matrix/JsSurface.vue", token: "jsUser", occurrence: 0 },
  },
  {
    id: "vue.matrix.js.completion.member",
    issue: "ISSUE-vue-matrix-js-completion",
    kind: "completion",
    file: "src/matrix/JsSurface.vue",
    completionOffsetNeedle: "jsUser.name",
    completionOffsetExtra: "jsUser.".length,
    completionLabels: ["name"],
  },
  {
    id: "vue.matrix.js.refs.bump",
    issue: "ISSUE-vue-matrix-js-refs",
    kind: "references",
    file: "src/matrix/JsSurface.vue",
    anchor: { file: "src/matrix/JsSurface.vue", token: "jsBump", occurrence: 0 },
    minRefs: 2,
  },
  {
    id: "vue.matrix.js.hover-range",
    issue: "ISSUE-vue-matrix-js-hover-range",
    kind: "hover-range",
    file: "src/matrix/JsSurface.vue",
    anchor: { file: "src/matrix/JsSurface.vue", token: "jsCount", occurrence: 2 },
  },

  // Teleport / suspense (presence + clean)
  {
    id: "vue.matrix.teleport-suspense.clean",
    issue: "ISSUE-vue-matrix-teleport",
    kind: "clean",
    file: "src/matrix/TeleportSuspense.vue",
  },

  // Mapping negatives
  {
    id: "vue.matrix.no-virtual.definition",
    issue: "ISSUE-vue-matrix-no-virtual",
    kind: "no-virtual-definition",
    file: "src/DailyBinding.vue",
    anchor: { file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 3 },
  },
  {
    id: "vue.matrix.no-virtual.component-tag",
    issue: "ISSUE-vue-matrix-no-virtual-tag",
    kind: "no-virtual-definition",
    file: "src/components/PropParent.vue",
    anchor: { file: "src/components/PropParent.vue", token: "PropChild", occurrence: 1 },
  },

  // Fallthrough extras
  {
    id: "vue.matrix.fallthrough.aria.clean-consumer",
    issue: "ISSUE-vue-matrix-ft-aria",
    kind: "clean",
    file: "src/fallthrough/DeepConsumer.vue",
  },

  // Scorecard densification (absolute clean surfaces)
  {
    id: "vue.matrix.strict-fallthrough.clean",
    issue: "ISSUE-vue-matrix-strict-ft-clean",
    kind: "clean",
    file: "src/strict/StrictFallthroughOk.vue",
  },
  {
    id: "vue.matrix.slot-correct.clean",
    issue: "ISSUE-vue-matrix-slot-correct-clean",
    kind: "clean",
    file: "src/slots/SlotCorrect.vue",
  },
  {
    id: "vue.matrix.ide-surface.clean",
    issue: "ISSUE-vue-matrix-ide-surface-clean",
    kind: "clean",
    file: "src/ide/IdeSurfaceParent.vue",
  },
  {
    id: "vue.matrix.js-daily.clean",
    issue: "ISSUE-vue-matrix-js-daily-clean",
    kind: "clean",
    file: "src/JsDaily.vue",
  },
  {
    id: "vue.matrix.generic-infer.clean",
    issue: "ISSUE-vue-matrix-generic-infer-clean",
    kind: "clean",
    file: "src/generics/GenericInferGood.vue",
  },

  // Macros surface clean
  {
    id: "vue.matrix.macros.surface.clean",
    issue: "ISSUE-vue-matrix-macros-clean",
    kind: "clean",
    file: "src/macros/MacroSurface.vue",
  },
  {
    id: "vue.matrix.generic.clean",
    issue: "ISSUE-vue-matrix-generic-clean",
    kind: "clean",
    file: "src/generics/GenericConsumer.vue",
  },
  {
    id: "vue.matrix.narrowing.clean",
    issue: "ISSUE-vue-matrix-narrowing-clean",
    kind: "clean",
    file: "src/features/Narrowing.vue",
  },
  {
    id: "vue.matrix.mapping.clean",
    issue: "ISSUE-vue-matrix-mapping-clean",
    kind: "clean",
    file: "src/features/MappingCase.vue",
  },
  {
    id: "vue.matrix.scoped.clean",
    issue: "ISSUE-vue-matrix-scoped-clean",
    kind: "clean",
    file: "src/features/ScopedStyle.vue",
  },
];

export const SVELTE_MATRIX_CASES: readonly MatrixCase[] = [
  {
    id: "svelte.matrix.directives.clean",
    issue: "ISSUE-svelte-matrix-directives-clean",
    kind: "clean",
    file: "src/matrix/Directives.svelte",
  },
  {
    id: "svelte.matrix.directives.if.hover",
    issue: "ISSUE-svelte-matrix-if-hover",
    kind: "hover",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "show", occurrence: 1 },
    needles: ["show"],
  },
  {
    id: "svelte.matrix.directives.each.hover",
    issue: "ISSUE-svelte-matrix-each-hover",
    kind: "hover",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "item", occurrence: 1 },
    needles: ["item"],
  },
  {
    id: "svelte.matrix.directives.bind.def",
    issue: "ISSUE-svelte-matrix-bind-def",
    kind: "definition",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "model", occurrence: 1 },
    target: { file: "src/matrix/Directives.svelte", token: "model", occurrence: 0 },
  },
  {
    id: "svelte.matrix.directives.class.hover",
    issue: "ISSUE-svelte-matrix-class-hover",
    kind: "hover",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "klass", occurrence: 1 },
    needles: ["klass"],
  },
  {
    id: "svelte.matrix.directives.style-color.hover",
    issue: "ISSUE-svelte-matrix-style-color",
    kind: "hover",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "color", occurrence: 1 },
    needles: ["color"],
  },
  {
    id: "svelte.matrix.directives.html.hover",
    issue: "ISSUE-svelte-matrix-html",
    kind: "hover",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "raw", occurrence: 1 },
    needles: ["raw"],
  },
  {
    id: "svelte.matrix.directives.onclick.def",
    issue: "ISSUE-svelte-matrix-onclick",
    kind: "definition",
    file: "src/matrix/Directives.svelte",
    anchor: { file: "src/matrix/Directives.svelte", token: "onClick", occurrence: 1 },
    target: { file: "src/matrix/Directives.svelte", token: "onClick", occurrence: 0 },
  },
  {
    id: "svelte.matrix.directives.completion",
    issue: "ISSUE-svelte-matrix-completion",
    kind: "completion",
    file: "src/matrix/Directives.svelte",
    completionOffsetNeedle: "{#if show}",
    completionOffsetExtra: "{#if ".length,
    completionLabels: ["show", "items", "model"],
  },

  {
    id: "svelte.matrix.events.clean",
    issue: "ISSUE-svelte-matrix-events-clean",
    kind: "clean",
    file: "src/matrix/EventsProps.svelte",
  },
  {
    id: "svelte.matrix.events.child-tag.def",
    issue: "ISSUE-svelte-matrix-events-tag",
    kind: "definition-file",
    file: "src/matrix/EventsProps.svelte",
    anchor: { file: "src/matrix/EventsProps.svelte", token: "EventChild", occurrence: 1 },
    targetFile: "src/matrix/EventChild.svelte",
  },
  {
    id: "svelte.matrix.events.handler.def",
    issue: "ISSUE-svelte-matrix-events-handler",
    kind: "definition",
    file: "src/matrix/EventsProps.svelte",
    anchor: { file: "src/matrix/EventsProps.svelte", token: "onPick", occurrence: 1 },
    target: { file: "src/matrix/EventsProps.svelte", token: "onPick", occurrence: 0 },
  },
  {
    id: "svelte.matrix.events.prop.hover",
    issue: "ISSUE-svelte-matrix-events-prop",
    kind: "hover",
    file: "src/matrix/EventsProps.svelte",
    anchor: { file: "src/matrix/EventsProps.svelte", token: "label", occurrence: 0 },
    needles: ["label"],
  },

  {
    id: "svelte.matrix.module.clean",
    issue: "ISSUE-svelte-matrix-module-clean",
    kind: "clean",
    file: "src/matrix/ModuleScript.svelte",
  },
  {
    id: "svelte.matrix.module.value.hover",
    issue: "ISSUE-svelte-matrix-module-hover",
    kind: "hover",
    file: "src/matrix/ModuleScript.svelte",
    anchor: { file: "src/matrix/ModuleScript.svelte", token: "value", occurrence: 1 },
    needles: ["value"],
  },

  {
    id: "svelte.matrix.js.clean",
    issue: "ISSUE-svelte-matrix-js-clean",
    kind: "clean",
    file: "src/matrix/JsSurface.svelte",
  },
  {
    id: "svelte.matrix.js.hover",
    issue: "ISSUE-svelte-matrix-js-hover",
    kind: "hover",
    file: "src/matrix/JsSurface.svelte",
    anchor: { file: "src/matrix/JsSurface.svelte", token: "jsCount", occurrence: 2 },
    needles: ["jsCount"],
  },
  {
    id: "svelte.matrix.js.def",
    issue: "ISSUE-svelte-matrix-js-def",
    kind: "definition",
    file: "src/matrix/JsSurface.svelte",
    anchor: { file: "src/matrix/JsSurface.svelte", token: "jsUser", occurrence: 1 },
    target: { file: "src/matrix/JsSurface.svelte", token: "jsUser", occurrence: 0 },
  },
  {
    id: "svelte.matrix.js.completion",
    issue: "ISSUE-svelte-matrix-js-completion",
    kind: "completion",
    file: "src/matrix/JsSurface.svelte",
    completionOffsetNeedle: "jsUser.name",
    completionOffsetExtra: "jsUser.".length,
    completionLabels: ["name"],
  },
  {
    id: "svelte.matrix.js.refs",
    issue: "ISSUE-svelte-matrix-js-refs",
    kind: "references",
    file: "src/matrix/JsSurface.svelte",
    anchor: { file: "src/matrix/JsSurface.svelte", token: "jsBump", occurrence: 0 },
    minRefs: 2,
  },
  {
    id: "svelte.matrix.js.hover-range",
    issue: "ISSUE-svelte-matrix-js-hover-range",
    kind: "hover-range",
    file: "src/matrix/JsSurface.svelte",
    anchor: { file: "src/matrix/JsSurface.svelte", token: "jsCount", occurrence: 2 },
  },

  {
    id: "svelte.matrix.no-virtual.definition",
    issue: "ISSUE-svelte-matrix-no-virtual",
    kind: "no-virtual-definition",
    file: "src/DailyBinding.svelte",
    anchor: { file: "src/DailyBinding.svelte", token: "dailyValue", occurrence: 3 },
  },
  {
    id: "svelte.matrix.runes.clean",
    issue: "ISSUE-svelte-matrix-runes-clean",
    kind: "clean",
    file: "src/runes/RunesSurface.svelte",
  },
  {
    id: "svelte.matrix.narrowing.clean",
    issue: "ISSUE-svelte-matrix-narrowing-clean",
    kind: "clean",
    file: "src/features/Narrowing.svelte",
  },
  {
    id: "svelte.matrix.mapping.clean",
    issue: "ISSUE-svelte-matrix-mapping-clean",
    kind: "clean",
    file: "src/features/MappingCase.svelte",
  },
  {
    id: "svelte.matrix.scoped.clean",
    issue: "ISSUE-svelte-matrix-scoped-clean",
    kind: "clean",
    file: "src/features/ScopedStyle.svelte",
  },
  {
    id: "svelte.matrix.bindable.clean",
    issue: "ISSUE-svelte-matrix-bindable-clean",
    kind: "clean",
    file: "src/features/BindableParent.svelte",
  },
  {
    id: "svelte.matrix.await.clean",
    issue: "ISSUE-svelte-matrix-await-clean",
    kind: "clean",
    file: "src/features/AwaitCase.svelte",
  },
  {
    id: "svelte.matrix.snippet.clean",
    issue: "ISSUE-svelte-matrix-snippet-clean",
    kind: "clean",
    file: "src/features/SnippetParent.svelte",
  },
  {
    id: "svelte.matrix.effect.clean",
    issue: "ISSUE-svelte-matrix-effect-clean",
    kind: "clean",
    file: "src/features/EffectCase.svelte",
  },
  {
    id: "svelte.matrix.strict-rest.clean",
    issue: "ISSUE-svelte-matrix-strict-rest-clean",
    kind: "clean",
    file: "src/strict/StrictRestOk.svelte",
  },
  {
    id: "svelte.matrix.js-daily.clean",
    issue: "ISSUE-svelte-matrix-js-daily-clean",
    kind: "clean",
    file: "src/JsDaily.svelte",
  },
  {
    id: "svelte.matrix.ide-surface.clean",
    issue: "ISSUE-svelte-matrix-ide-surface-clean",
    kind: "clean",
    file: "src/ide/IdeSurfaceParent.svelte",
  },
  {
    id: "svelte.matrix.snippet-correct.clean",
    issue: "ISSUE-svelte-matrix-snippet-correct-clean",
    kind: "clean",
    file: "src/slots/SnippetCorrect.svelte",
  },
  {
    id: "svelte.matrix.generic-infer.clean",
    issue: "ISSUE-svelte-matrix-generic-infer-clean",
    kind: "clean",
    file: "src/generics/GenericInferGood.svelte",
  },
];
