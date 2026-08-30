/**
 * @ai-generated - Verifies native component-meta payload mapping preserves the full public metadata contract.
 */

import { describe, expect, it } from "vitest";
import {
  nativeComponentMetaToComponentMeta as mapNativeComponentMeta,
  nativeTypeRegistryToMap,
} from "./native-component-meta.js";

function nativeComponentMetaToComponentMeta(meta: any) {
  return mapNativeComponentMeta({
    orderedSfcStructure: {
      schemaVersion: 1,
      artifactToken: "a".repeat(43),
      blocks: [],
      markupNodes: [],
    },
    ...meta,
  });
}

/** Default fallthrough surface fields for test payloads. */
const defaultFallthroughFields = {
  acceptedProps: [] as any[],
  acceptedEvents: [] as any[],
  acceptedSurfaceCompleteness: "exact" as const,
  rootReachability: { kind: "noFallthrough" as const, reason: "noTemplate" as const },
  fallthroughSurface: { kind: "none" as const, reason: "noTemplate" as const },
};

function publishedTypeFields(text?: string) {
  return {
    publication: {
      kind: "published" as const,
      semanticAuthority: "resolved" as const,
      exactness: "exactConcrete" as const,
      reason: { kind: "resolvedExactConcrete" as const },
      provenance: { kind: "resolved" as const, value: "semanticEvaluator" as const },
    },
    terminalDisplay: { text },
  };
}

function typeDeclaration(canonicalSource: string, name: string) {
  return {
    requestedName: name,
    resolvedName: name,
    canonicalSource,
    spanStart: 0,
    spanEnd: 1,
    kind: "typeAlias" as const,
  };
}

function nativeMetaWithProp(prop: Record<string, unknown>) {
  return {
    filePath: "/project/src/App.vue",
    componentPublicContract: {
      kind: "unsupported" as const,
      adapterId: "vue",
      reason: { kind: "componentMetaUnavailable" as const },
      diagnostics: [],
    },
    optionsApi: false,
    props: [prop],
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
    orderedSfcStructure: {
      schemaVersion: 1,
      artifactToken: "a".repeat(43),
      blocks: [],
      markupNodes: [],
    },
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
}

describe("nativeComponentMetaToComponentMeta", () => {
  it("exposes mandatory contract availability on ComponentMeta", () => {
    const native = nativeMetaWithProp({
      name: "value",
      type: { kind: "primitive", name: "string" },
      ...publishedTypeFields("string"),
      required: true,
      hasDefault: false,
    });

    const mapped = nativeComponentMetaToComponentMeta(native);

    expect(mapped.componentPublicContract).toBe(native.componentPublicContract);
  });

  it("projects compat rawType only from terminal display and rejects Failed publication", () => {
    const published = nativeComponentMetaToComponentMeta(
      nativeMetaWithProp({
        name: "value",
        type: { kind: "primitive", name: "string" },
        ...publishedTypeFields("TerminalDisplayOnly"),
        required: true,
        hasDefault: false,
      }),
    );

    expect(published.props[0].rawType).toBe("TerminalDisplayOnly");
    const perturbed = nativeComponentMetaToComponentMeta(
      nativeMetaWithProp({
        name: "value",
        type: { kind: "primitive", name: "string" },
        ...publishedTypeFields("HOSTILE_UNION | HOSTILE_ARRAY[]"),
        required: true,
        hasDefault: false,
      }),
    );
    expect(perturbed.props[0].type).toEqual(published.props[0].type);
    expect(perturbed.props[0].rawType).not.toBe(published.props[0].rawType);
    expect(() =>
      nativeComponentMetaToComponentMeta(
        nativeMetaWithProp({
          name: "value",
          type: { kind: "primitive", name: "string" },
          publication: {
            kind: "failed",
            failure: "unrepresentableRequiredMemberValue",
            provenance: "semanticEvaluator",
          },
          terminalDisplay: {},
          required: true,
          hasDefault: false,
        }),
      ),
    ).toThrow(/failed type publication/i);
  });

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
          ...publishedTypeFields("string"),
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
          ...publishedTypeFields("Items"),
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
          ...publishedTypeFields("Payload"),
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
              ...publishedTypeFields("Row"),
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
          ...publishedTypeFields("string"),
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
          bindings: [
            {
              name: "item",
              type: { kind: "primitive", name: "number" },
              ...publishedTypeFields("number"),
            },
          ],
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

  it("maps the mandatory ordered structure from the native payload", () => {
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
      orderedSfcStructure: {
        schemaVersion: 1,
        artifactToken: "artifact-token",
        blocks: [],
        markupNodes: [],
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

    expect(meta.orderedSfcStructure).toMatchObject({
      schemaVersion: 1,
      artifactToken: "artifact-token",
      blocks: [],
      markupNodes: [],
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

  it("keeps a ratified indexed-access publication shallow beside the native registry", () => {
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
          ...publishedTypeFields("NuxtLinkProps['to']"),
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
          declaration: typeDeclaration("/project/src/link-types.ts", "NuxtLinkProps"),
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
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "NuxtLinkProps" },
      indexType: { kind: "literal", value: "to" },
    });

    const registry = nativeTypeRegistryToMap(native);
    expect(registry?.get("NuxtLinkProps")).toBeDefined();
    expect(registry?.get("RouteLocationRaw")).toBeDefined();

    native.typeRegistry[0].declaration = typeDeclaration("/project/src/Link.vue", "NuxtLinkProps");
    const local = nativeComponentMetaToComponentMeta(native);
    expect(local.props[0]?.type).toEqual({
      kind: "union",
      types: [
        { kind: "ref", name: "RouteLocationRaw" },
        { kind: "primitive", name: "undefined" },
      ],
    });
  });

  it("keeps a ratified generic ref publication shallow beside its registry body", () => {
    const native = nativeMetaWithProp({
      name: "valueKey",
      type: {
        kind: "ref",
        name: "GetItemKeys",
        typeArguments: [{ kind: "ref", name: "T", typeArguments: [] }],
      },
      ...publishedTypeFields("GetItemKeys<T>"),
      required: false,
      hasDefault: true,
    });
    native.typeRegistry = [
      {
        name: "GetItemKeys",
        declaration: typeDeclaration("/project/src/helpers.ts", "GetItemKeys"),
        type: {
          kind: "union",
          types: [
            { kind: "primitive", name: "string" },
            { kind: "primitive", name: "number" },
          ],
        },
      },
    ];

    const compat = nativeComponentMetaToComponentMeta(native);

    expect(compat.props[0]?.type).toEqual({
      kind: "ref",
      name: "GetItemKeys",
      typeArguments: [{ kind: "ref", name: "T" }],
    });
    expect(nativeTypeRegistryToMap(native)?.get("GetItemKeys")).toEqual({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
  });

  it("maps an exact, a proven-non-wrapper and a typed-degraded return wrapper role distinctly", () => {
    const native = nativeMetaWithProp({
      name: "value",
      type: { kind: "primitive", name: "string" },
      ...publishedTypeFields("string"),
      required: true,
      hasDefault: false,
    });
    native.bindings = [
      {
        name: "counter",
        kind: "const",
        reactivityKind: "ref",
        returnWrapperRole: "ref",
        usedInTemplate: true,
        usedInStyle: false,
      },
      {
        name: "plain",
        kind: "const",
        reactivityKind: "maybeRef",
        returnWrapperRole: "none",
        usedInTemplate: false,
        usedInStyle: false,
      },
      {
        name: "degraded",
        kind: "const",
        reactivityKind: "maybeRef",
        returnWrapperRole: "unresolved",
        returnWrapperUnresolvedReason: "cycle",
        usedInTemplate: false,
        usedInStyle: false,
      },
      {
        name: "undemanded",
        kind: "const",
        reactivityKind: "maybeRef",
        usedInTemplate: false,
        usedInStyle: false,
      },
    ];

    const mapped = nativeComponentMetaToComponentMeta(native);

    // EXACT — the role rides alongside the refined decoration kind.
    expect(mapped.bindings).toEqual([
      {
        name: "counter",
        kind: "const",
        reactivityKind: "ref",
        returnWrapperRole: "ref",
        usedInTemplate: true,
        usedInStyle: false,
      },
      // COMPLETED NON-WRAPPER PROOF — published as `"none"` while the
      // decoration kind stays at the undecided `maybeRef`.
      {
        name: "plain",
        kind: "const",
        reactivityKind: "maybeRef",
        returnWrapperRole: "none",
        usedInTemplate: false,
        usedInStyle: false,
      },
      // TYPED DEGRADATION — the exact reason survives the mapping and does NOT
      // collapse onto the bare `"unresolved"` discriminant.
      {
        name: "degraded",
        kind: "const",
        reactivityKind: "maybeRef",
        returnWrapperRole: "unresolved",
        returnWrapperUnresolvedReason: "cycle",
        usedInTemplate: false,
        usedInStyle: false,
      },
      // UNDEMANDED — both keys stay ABSENT, never `undefined`-valued and never
      // conflated with the `"none"` proof.
      {
        name: "undemanded",
        kind: "const",
        reactivityKind: "maybeRef",
        usedInTemplate: false,
        usedInStyle: false,
      },
    ]);
    expect("returnWrapperRole" in mapped.bindings[3]!).toBe(false);
    expect("returnWrapperUnresolvedReason" in mapped.bindings[0]!).toBe(false);
    expect(mapped.bindings[1]!.returnWrapperRole).not.toBe(mapped.bindings[3]!.returnWrapperRole);
  });
});
