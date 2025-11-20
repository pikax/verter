import * as core from "@vue/language-core";
import * as ts from "typescript";
import * as CompilerDOM from "@vue/compiler-dom";

const file = `
<script setup lang="ts">
import { defineProps } from 'vue'
const foo = 1;
</script>
<template>
  <div>   
    {{foo}}
  </div>
</template>
`;

const noop = () => {};
export function getDefaultCompilerOptions(
  target = 99,
  lib = "vue",
  strictTemplates = false
) {
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

const snap = ts.ScriptSnapshot.fromString(file);
const plugins = core.createPlugins({
  modules: {
    typescript: ts,
    "@vue/compiler-dom": CompilerDOM,
  },
  vueCompilerOptions: getDefaultCompilerOptions(),
});
new core.VueVirtualCode("foo.vue", "vue", snap, {}, plugins, ts);
