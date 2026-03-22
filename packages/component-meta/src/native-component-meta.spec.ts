/**
 * @ai-generated - Verifies native component-meta payload mapping preserves the full public metadata contract.
 */

import { describe, expect, it } from "vitest";
import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "./native-component-meta.js";

/** Default fallthrough surface fields for test payloads. */
const defaultFallthroughFields = {
  acceptedProps: [] as any[],
  acceptedEvents: [] as any[],
  acceptedSurfaceCompleteness: "exact" as const,
  rootReachability: { kind: "noFallthrough" as const, reason: "noTemplate" as const },
  fallthroughSurface: { kind: "none" as const, reason: "noTemplate" as const },
};

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
      ...defaultFallthroughFields,
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
        ...defaultFallthroughFields,
      } as any),
    ).toThrow();
  });

  it("ignores native-only resolution sidecars and keeps compat public-only", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "visible",
          type: { kind: "primitive", name: "string" },
          rawType: "string",
          required: true,
          hasDefault: false,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      components: [],
      templateRefs: [],
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
      ...defaultFallthroughFields,
      resolution: {
        mode: "expanded",
        macros: [
          {
            macroIndex: 0,
            macroKind: "DefineProps",
            typeName: "Props",
            importSource: "./types",
            declaration: {
              requestedName: "Props",
              resolvedName: "Props",
              canonicalSource: "/project/src/types.ts",
              spanStart: 10,
              spanEnd: 50,
              kind: "class",
              text: "export class Props { visible!: string; protected hidden!: boolean; private secret!: symbol }",
            },
            nativeProps: [
              {
                name: "visible",
                isOptional: false,
                typeAnnotation: "string",
                visibility: "public",
                spanStart: 30,
                spanEnd: 45,
              },
              {
                name: "hidden",
                isOptional: false,
                typeAnnotation: "boolean",
                visibility: "protected",
                spanStart: 46,
                spanEnd: 64,
              },
              {
                name: "secret",
                isOptional: false,
                typeAnnotation: "symbol",
                visibility: "private",
                spanStart: 65,
                spanEnd: 83,
              },
            ],
            props: [],
            emits: [],
            slots: [],
          },
        ],
      },
    } as any);

    expect(meta.props.map((prop) => prop.name)).toEqual(["visible"]);
  });

  it("preserves raw type-registry provenance while compat mapping still uses the expanded type", () => {
    const native = {
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      typeRegistry: [
        {
          name: "Props",
          type: {
            kind: "object",
            properties: [
              { name: "label", optional: false, type: { kind: "primitive", name: "string" } },
            ],
          },
          rawType: "export interface Props { label: string }",
          declaration: {
            requestedName: "Props",
            resolvedName: "Props",
            canonicalSource: "/project/src/types.ts",
            spanStart: 12,
            spanEnd: 48,
            kind: "interface",
            text: "export interface Props { label: string }",
          },
        },
      ],
      components: [],
      templateRefs: [],
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
      ...defaultFallthroughFields,
    } as any;

    expect(native.typeRegistry[0].rawType).toContain("export interface Props");
    expect(native.typeRegistry[0].declaration.text).toContain("export interface Props");

    const registry = nativeTypeRegistryToMap(native);
    expect(registry?.get("Props")).toBeDefined();

    const compat = nativeComponentMetaToComponentMeta(native);
    expect(compat.props).toEqual([]);
  });

  it("preserves structured fallthrough reason payloads", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      components: [],
      templateRefs: [],
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
      acceptedProps: [],
      acceptedEvents: [],
      acceptedSurfaceCompleteness: "lowerBound",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: {
        kind: "branches",
        branches: [
          {
            branchKey: "0",
            props: [],
            events: [],
            rootChain: [
              {
                kind: "unresolved",
                tag: "component",
                reason: { kind: "cycle", canonicalId: "/project/src/App.vue" },
              },
            ],
            status: {
              kind: "partiallyUnresolved",
              reasons: [{ kind: "unknownSpread" }],
            },
          },
          {
            branchKey: "1",
            props: [],
            events: [],
            rootChain: [],
            status: {
              kind: "unresolved",
              reason: {
                kind: "unresolvedChildImport",
                importSource: "./Child.vue",
              },
            },
          },
        ],
      },
    } as any);

    expect(meta.acceptedSurfaceCompleteness).toBe("lowerBound");
    expect(meta.fallthroughSurface).toEqual({
      kind: "branches",
      branches: [
        {
          branchKey: "0",
          props: [],
          events: [],
          rootChain: [
            {
              kind: "unresolved",
              tag: "component",
              reason: { kind: "cycle", canonicalId: "/project/src/App.vue" },
            },
          ],
          status: {
            kind: "partiallyUnresolved",
            reasons: [{ kind: "unknownSpread" }],
          },
        },
        {
          branchKey: "1",
          props: [],
          events: [],
          rootChain: [],
          status: {
            kind: "unresolved",
            reason: {
              kind: "unresolvedChildImport",
              importSource: "./Child.vue",
            },
          },
        },
      ],
    });
  });
});
