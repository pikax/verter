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
          props: [
            {
              name: "label",
              isBound: true,
              constness: "dynamic",
              expression: "labelExpr",
              referencedBindings: ["labelRef"],
              fromSpread: false,
              isShorthand: false,
            },
          ],
          hasSpread: false,
          slotsUsed: ["default"],
          staticClasses: ["primary"],
          hasDynamicClass: true,
          vModels: ["modelValue"],
          vModelEntries: [{ bindingName: "modelValue" }],
        },
      ],
      templateRefs: [{ name: "button", isDynamic: false, targetTag: "FancyButton" }],
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", kind: "named", importedName: null, isTypeOnly: false }],
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
        props: [
          {
            name: "label",
            isBound: true,
            constness: "dynamic",
            expression: "labelExpr",
            referencedBindings: ["labelRef"],
            fromSpread: false,
            isShorthand: false,
          },
        ],
        hasSpread: false,
        slotsUsed: ["default"],
        staticClasses: ["primary"],
        hasDynamicClass: true,
        vModels: ["modelValue"],
        vModelEntries: [{ bindingName: "modelValue" }],
        bindings: [],
        events: [],
      },
    ]);
    expect(meta.templateRefs).toEqual([
      { name: "button", isDynamic: false, targetTag: "FancyButton" },
    ]);
    expect(meta.imports).toEqual([
      {
        source: "vue",
        isTypeOnly: false,
        bindings: [{ name: "ref", kind: "named", importedName: null, isTypeOnly: false }],
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

  it("preserves native expansion metadata on mapped public members", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "items",
          type: { kind: "ref", name: "Items", typeArguments: [] },
          typeExpansion: {
            exactness: "incomplete",
            executionStatus: "completed",
            diagnostics: [
              {
                reason: "unresolvedReference",
                context: "unresolved type reference 'Items'",
              },
            ],
          },
          required: false,
          hasDefault: false,
        },
      ],
      events: [
        {
          name: "select",
          payload: { kind: "ref", name: "Payload", typeArguments: [] },
          payloadExpansion: {
            exactness: "incomplete",
            executionStatus: "completed",
            diagnostics: [
              {
                reason: "unsupportedOperator",
                context: "indexed access was preserved symbolically",
              },
            ],
          },
        },
      ],
      slots: [
        {
          name: "default",
          isScoped: true,
          isRequired: false,
          bindings: [
            {
              name: "row",
              type: { kind: "ref", name: "Row", typeArguments: [] },
              typeExpansion: {
                exactness: "incomplete",
                executionStatus: "completed",
                diagnostics: [
                  {
                    reason: "mappedDepthExceeded",
                    context: "mapped type was preserved symbolically",
                  },
                ],
              },
            },
          ],
        },
      ],
      models: [],
      exposed: [
        {
          name: "api",
          type: { kind: "ref", name: "Api", typeArguments: [] },
          typeExpansion: {
            exactness: "incomplete",
            executionStatus: "completed",
            diagnostics: [
              {
                reason: "budgetExceeded",
                context: "symbolic work limit reached during normalization",
              },
            ],
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
    } as any);

    expect(meta.props[0].typeExpansion?.exactness).toBe("incomplete");
    expect(meta.events[0].payloadExpansion?.exactness).toBe("incomplete");
    expect(meta.slots[0].bindings[0].typeExpansion?.exactness).toBe("incomplete");
    expect(meta.exposed[0].typeExpansion?.exactness).toBe("incomplete");
    expect(meta.props[0].typeExpansion?.executionStatus).toBe("completed");
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

  it("maps additive public-instance members separately from defineExpose metadata", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: { kind: "primitive", name: "string" },
          required: true,
          hasDefault: false,
        },
      ],
      events: [],
      slots: [
        {
          name: "default",
          isScoped: true,
          isRequired: false,
          bindings: [{ name: "item", type: { kind: "primitive", name: "number" } }],
        },
      ],
      models: [],
      exposed: [
        {
          name: "focus",
          type: { kind: "primitive", name: "number" },
        },
      ],
      publicInstance: {
        completeness: "partial",
        members: [
          {
            name: "label",
            kind: "prop",
            type: { kind: "primitive", name: "string" },
          },
          {
            name: "$slots",
            kind: "slotContainer",
            type: {
              kind: "object",
              properties: [
                {
                  name: "default",
                  optional: true,
                  type: { kind: "primitive", name: "unknown" },
                },
              ],
            },
          },
          {
            name: "focus",
            kind: "exposed",
            type: { kind: "primitive", name: "number" },
          },
        ],
      },
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
    } as any);

    expect(meta.exposed.map((member) => member.name)).toEqual(["focus"]);
    expect(meta.publicInstance?.members.map((member) => member.name)).toEqual([
      "label",
      "$slots",
      "focus",
    ]);
    expect(meta.publicInstance?.members[1]).toMatchObject({
      name: "$slots",
      kind: "slotContainer",
    });
  });

  it("maps additive SFC block metadata from the native payload", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      publicInstance: {
        completeness: "partial",
        members: [],
      },
      sfcBlocks: {
        script: {
          lang: "ts",
          attributes: [{ name: "lang", value: "ts" }],
        },
        scriptSetup: {
          lang: "ts",
          generic: "T extends string = string",
          attrsType: "ButtonAttrs",
          attributes: [
            { name: "setup" },
            { name: "lang", value: "ts" },
            { name: "generic", value: "T extends string = string" },
            { name: "attrs", value: "ButtonAttrs" },
          ],
        },
        template: {
          lang: "html",
          attributes: [
            { name: "lang", value: "html" },
            { name: "data-layout", value: "stack" },
          ],
        },
        styles: [
          {
            index: 0,
            lang: "scss",
            scoped: true,
            isModule: true,
            moduleName: "theme",
            attributes: [
              { name: "scoped" },
              { name: "module", value: "theme" },
              { name: "lang", value: "scss" },
            ],
          },
        ],
        custom: [
          {
            index: 0,
            blockType: "i18n",
            lang: "json",
            attributes: [{ name: "lang", value: "json" }],
          },
        ],
      },
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
    } as any);

    expect(meta.sfcBlocks).toEqual({
      script: {
        lang: "ts",
        attributes: [{ name: "lang", value: "ts" }],
      },
      scriptSetup: {
        lang: "ts",
        generic: "T extends string = string",
        attrsType: "ButtonAttrs",
        attributes: [
          { name: "setup" },
          { name: "lang", value: "ts" },
          { name: "generic", value: "T extends string = string" },
          { name: "attrs", value: "ButtonAttrs" },
        ],
      },
      template: {
        lang: "html",
        attributes: [
          { name: "lang", value: "html" },
          { name: "data-layout", value: "stack" },
        ],
      },
      styles: [
        {
          index: 0,
          lang: "scss",
          scoped: true,
          isModule: true,
          moduleName: "theme",
          attributes: [
            { name: "scoped" },
            { name: "module", value: "theme" },
            { name: "lang", value: "scss" },
          ],
        },
      ],
      custom: [
        {
          index: 0,
          blockType: "i18n",
          lang: "json",
          attributes: [{ name: "lang", value: "json" }],
        },
      ],
    });
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

  it("derives a direct root summary from root reachability branches", () => {
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
      acceptedSurfaceCompleteness: "exact",
      rootReachability: {
        kind: "branches",
        branches: [
          {
            branchIndex: 0,
            target: {
              kind: "componentUsage",
              elementIndex: 1,
              usageIndex: 0,
              name: "PrimaryButton",
              importSource: "./PrimaryButton.vue",
            },
            consumed: {
              attrs: ["class"],
              listeners: ["click"],
              hasDynamicAttrName: false,
              hasDynamicListenerName: false,
            },
            hasUnknownSpread: false,
          },
          {
            branchIndex: 1,
            conditionText: "isFallback",
            target: {
              kind: "nativeElement",
              elementIndex: 2,
              tag: "button",
            },
            consumed: {
              attrs: [],
              listeners: [],
              hasDynamicAttrName: false,
              hasDynamicListenerName: false,
            },
            hasUnknownSpread: false,
          },
        ],
      },
      fallthroughSurface: {
        kind: "branches",
        branches: [],
      },
    } as any);

    expect(meta.rootInfo).toEqual({
      kind: "conditional",
      targets: [
        {
          kind: "componentUsage",
          elementIndex: 1,
          usageIndex: 0,
          name: "PrimaryButton",
          importSource: "./PrimaryButton.vue",
        },
        {
          kind: "nativeElement",
          elementIndex: 2,
          tag: "button",
        },
      ],
    });
  });

  it("surfaces slot returnType from the native payload", () => {
    const meta = nativeComponentMetaToComponentMeta({
      filePath: "/project/src/App.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [
        {
          name: "default",
          isScoped: true,
          bindings: [],
          isRequired: false,
          returnType: "VNode[]",
        },
      ],
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
    } as any);

    expect(meta.slots).toEqual([
      {
        name: "default",
        isScoped: true,
        bindings: [],
        isRequired: false,
        returnType: "VNode[]",
        // Forward-compat coercion: a native payload without the producer
        // fact reads `false` (the compat name block applies).
        declaredInMacroTypeArg: false,
      },
    ]);
  });

  it("resolves indexed-access prop descriptors through the native type registry", () => {
    const native = {
      filePath: "/project/src/Link.vue",
      optionsApi: false,
      props: [
        {
          name: "href",
          type: {
            kind: "indexedAccess",
            object: { kind: "ref", name: "NuxtLinkProps", typeArguments: [] },
            index: { kind: "literal", literalKind: "string", value: "to" },
          },
          rawType: "NuxtLinkProps['to']",
          required: false,
          hasDefault: false,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      typeRegistry: [
        {
          name: "NuxtLinkProps",
          type: {
            kind: "object",
            properties: [
              {
                memberKind: "property",
                name: "to",
                ty: { kind: "ref", name: "RouteLocationRaw", typeArguments: [] },
                optional: true,
                readonly: false,
              },
            ],
          },
        },
        {
          name: "RouteLocationRaw",
          type: {
            kind: "union",
            types: [
              { kind: "primitive", name: "string" },
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "path",
                    ty: { kind: "primitive", name: "string" },
                    optional: false,
                    readonly: false,
                  },
                ],
              },
            ],
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

    const compat = nativeComponentMetaToComponentMeta(native);
    expect(compat.props[0]?.type).toEqual({
      kind: "union",
      types: [
        { kind: "ref", name: "RouteLocationRaw" },
        { kind: "primitive", name: "undefined" },
      ],
    });

    const registry = nativeTypeRegistryToMap(native);
    expect(registry?.get("NuxtLinkProps")).toBeDefined();
    expect(registry?.get("RouteLocationRaw")).toBeDefined();
  });
});
