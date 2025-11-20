import * as core from "@vue/language-core";
import * as ts from "typescript";
import * as CompilerDOM from "@vue/compiler-dom";
import * as verter from "@verter/core";

const noop = () => {};

export function getDefaultCompilerOptions(
  target = 99,
  lib = "vue",
  strictTemplates = false
): core.VueCompilerOptions {
  return {
    target,
    lib,
    globalTypesPath: noop,
    extensions: [".vue"],
    vitePressExtensions: [],
    petiteVueExtensions: [],
    jsxSlots: false,
    strictVModel: strictTemplates,
    strictCssModules: false,
    checkUnknownProps: strictTemplates,
    checkUnknownEvents: strictTemplates,
    checkUnknownDirectives: strictTemplates,
    checkUnknownComponents: strictTemplates,
    inferComponentDollarEl: false,
    inferComponentDollarRefs: false,
    inferTemplateDollarAttrs: false,
    inferTemplateDollarEl: false,
    inferTemplateDollarRefs: false,
    inferTemplateDollarSlots: false,
    skipTemplateCodegen: false,
    fallthroughAttributes: false,
    resolveStyleImports: false,
    resolveStyleClassNames: "scoped",
    fallthroughComponentNames: [
      "Transition",
      "KeepAlive",
      "Teleport",
      "Suspense",
    ],
    dataAttributes: [],
    htmlAttributes: ["aria-*"],
    optionsWrapper: [`(await import('${lib}')).defineComponent(`, `)`],
    macros: {
      defineProps: ["defineProps"],
      defineSlots: ["defineSlots"],
      defineEmits: ["defineEmits"],
      defineExpose: ["defineExpose"],
      defineModel: ["defineModel"],
      defineOptions: ["defineOptions"],
      withDefaults: ["withDefaults"],
    },
    composables: {
      useAttrs: ["useAttrs"],
      useCssModule: ["useCssModule"],
      useSlots: ["useSlots"],
      useTemplateRef: ["useTemplateRef", "templateRef"],
    },
    plugins: [],
    experimentalModelPropName: {
      "": {
        input: true,
      },
      value: {
        input: { type: "text" },
        textarea: true,
        select: true,
      },
    },
  };
}

const VolarCompilerOptions = getDefaultCompilerOptions();
const VolarPlugins = core.createPlugins({
  modules: {
    typescript: ts,
    "@vue/compiler-dom": CompilerDOM,
  },
  vueCompilerOptions: VolarCompilerOptions,
  compilerOptions: {},
});
export function parseStringVolar(content: string) {
  const snap = ts.ScriptSnapshot.fromString(content);
  return new core.VueVirtualCode(
    "foo.vue",
    "vue",
    snap,
    VolarCompilerOptions,
    VolarPlugins,
    ts
  );
}

export function parseStringVerter(content: string) {
  const parsed = verter.parser(content);
  return parsed;
}
