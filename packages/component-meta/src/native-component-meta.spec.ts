/**
 * @ai-generated - Verifies native component-meta payload mapping preserves the full public metadata contract.
 */

import { describe, expect, it } from "vitest";
import { nativeComponentMetaToComponentMeta } from "./native-component-meta.js";

describe("nativeComponentMetaToComponentMeta", () => {
  it("preserves full metadata fields from the native result", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      components: [
        {
          name: "FancyButton",
          importSource: "./FancyButton.vue",
          isDynamic: false,
          props: [{ name: "label", isBound: true, constness: "dynamic" }],
          slotsUsed: ["default"],
          staticClasses: ["primary"],
          hasDynamicClass: true,
          vModels: ["modelValue"],
        },
      ],
      templateRefs: [{ name: "button", isDynamic: false, targetTag: "FancyButton" }],
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", isTypeOnly: false }],
        },
      ],
      bindings: [
        {
          name: "count",
          kind: "const",
          reactivityKind: "ref",
          typeAnnotation: "Ref<number>",
          usedInTemplate: true,
          usedInStyle: false,
        },
      ],
      vueApiCalls: [{ api: "OnMounted", argValue: "button" }],
      styles: [
        {
          lang: "Css",
          scoped: true,
          isModule: true,
          moduleName: "theme",
          classes: ["primary"],
          ids: ["wrapper"],
          customProperties: ["--accent"],
          vBinds: ["accentColor"],
          selectors: [{ text: ".primary", specificity: [0, 1, 0] }],
        },
      ],
      flags: {
        asyncSetup: false,
        hasReactiveState: true,
        hasComputed: false,
        hasWatchers: false,
        hasLifecycleHooks: true,
        hasProvide: false,
        hasInject: false,
        hasInheritAttrsFalse: false,
        hasStoreUsage: false,
      },
    } as any);

    expect(meta.components).toEqual([
      {
        name: "FancyButton",
        importSource: "./FancyButton.vue",
        isDynamic: false,
        props: [{ name: "label", isBound: true, constness: "dynamic" }],
        slotsUsed: ["default"],
        staticClasses: ["primary"],
        hasDynamicClass: true,
        vModels: ["modelValue"],
      },
    ]);
    expect(meta.templateRefs).toEqual([
      { name: "button", isDynamic: false, targetTag: "FancyButton" },
    ]);
    expect(meta.imports).toEqual([
      {
        source: "vue",
        isTypeOnly: false,
        bindings: [{ name: "ref", isTypeOnly: false }],
      },
    ]);
    expect(meta.bindings).toEqual([
      {
        name: "count",
        kind: "const",
        reactivityKind: "ref",
        typeAnnotation: "Ref<number>",
        usedInTemplate: true,
        usedInStyle: false,
      },
    ]);
    expect(meta.vueApiCalls).toEqual([{ api: "OnMounted", argValue: "button" }]);
    expect(meta.styles).toEqual([
      {
        lang: "Css",
        scoped: true,
        isModule: true,
        moduleName: "theme",
        classes: ["primary"],
        ids: ["wrapper"],
        customProperties: ["--accent"],
        vBinds: ["accentColor"],
        selectors: [{ text: ".primary", specificity: [0, 1, 0] }],
      },
    ]);
  });

  it("does not fabricate missing full-metadata arrays", () => {
    expect(() =>
      nativeComponentMetaToComponentMeta({
        filePath: "/project/src/App.vue",
        optionsApi: false,
        props: [],
        events: [],
        slots: [],
        models: [],
        exposed: [],
        imports: [],
        bindings: [],
        vueApiCalls: [],
        styles: [],
        flags: {
          asyncSetup: false,
          hasReactiveState: false,
          hasComputed: false,
          hasWatchers: false,
          hasLifecycleHooks: false,
          hasProvide: false,
          hasInject: false,
          hasInheritAttrsFalse: false,
          hasStoreUsage: false,
        },
      } as any),
    ).toThrow();
  });
});
