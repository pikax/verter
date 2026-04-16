import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { describe, it, expect, vi } from "vitest";
import {
  ComponentMetaChecker,
  createChecker,
  createCheckerByJson,
  mapPropMeta,
  mapEventMeta,
  mapSlotMeta,
  mapExposedMeta,
  mapComponentMeta,
} from "./checker.js";
import { openComponentMetaSession } from "../project.js";
import {
  getMetaRuntime,
  normalizePath as runtimeNormalizePath,
  shutdownMetaRuntime,
} from "../runtime/index.js";
import { resolvePath } from "../runtime/engine-key.js";
import { array, func, literal, object, primitive, ref, tuple, union, unknown } from "../type-ir.js";
import type { PropMeta, EventMeta, SlotMeta, ExposedMeta } from "../types.js";

let nextProjectRootId = 1;

async function createRuntimeChecker(
  name = "component-meta-checker",
): Promise<ComponentMetaChecker> {
  const projectRoot = resolve(process.env.TEMP ?? "/tmp", `${name}-${nextProjectRootId++}`);
  return createCheckerByJson(projectRoot, {});
}

async function settleNativeProject(): Promise<void> {
  await new Promise<void>((resolve) => setImmediate(resolve));
}

function nativeMetaPayload(filePath: string) {
  return {
    filePath,
    optionsApi: false,
    props: [
      {
        name: "label",
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
    acceptedProps: [],
    acceptedEvents: [],
    acceptedSurfaceCompleteness: "exact",
    rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
    fallthroughSurface: { kind: "none", reason: "noTemplate" },
  };
}

// ── Mapper unit tests ───────────────────────────────────────────────

describe("mapPropMeta", () => {
  it("maps a prop with description and tags", () => {
    const prop: PropMeta = {
      name: "label",
      type: primitive("string"),
      required: true,
      hasDefault: false,
      rawType: "string",
      description: "The label text",
      tags: [{ name: "default", text: "'hello'" }],
    };

    const result = mapPropMeta(prop);

    expect(result.name).toBe("label");
    expect(result.description).toBe("The label text");
    expect(result.type).toBe("string");
    expect(result.required).toBe(true);
    expect(result.global).toBe(false);
    expect(result.tags).toEqual([{ name: "default", text: "'hello'" }]);
    expect(result.schema).toBe("string");
  });

  it("defaults description to empty string when missing", () => {
    const prop: PropMeta = {
      name: "count",
      type: primitive("number"),
      required: false,
      hasDefault: true,
    };

    const result = mapPropMeta(prop);

    expect(result.description).toBe("");
    expect(result.tags).toEqual([]);
  });

  it("prefers raw indexed-access text over opaque graph placeholders", () => {
    const prop: PropMeta = {
      name: "label",
      type: unknown("graphNode(13)"),
      required: true,
      hasDefault: false,
      rawType: 'Fields["label"]',
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe('Fields["label"]');
  });

  it("normalizes simple string defaults to JSON-style quoted strings", () => {
    const prop: PropMeta = {
      name: "type",
      type: union([primitive("string"), primitive("undefined")]),
      required: false,
      hasDefault: true,
      default: "'single'",
      rawType: "string | undefined",
    };

    const result = mapPropMeta(prop);

    expect(result.default).toBe('"single"');
  });

  it("keeps top-level undefined in optional prop display text", () => {
    const prop: PropMeta = {
      name: "modelValue",
      type: union([primitive("string"), array(primitive("string")), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "string | string[] | undefined",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("string | string[] | undefined");
  });

  it("adds undefined to optional prop display text when raw type omits it", () => {
    const prop: PropMeta = {
      name: "active",
      type: primitive("boolean"),
      required: false,
      hasDefault: false,
      rawType: "boolean",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("boolean | undefined");
  });

  it("adds undefined to optional prop schema when native descriptors omit it", () => {
    const prop: PropMeta = {
      name: "active",
      type: primitive("boolean"),
      required: false,
      hasDefault: false,
      rawType: "boolean",
    };

    const result = mapPropMeta(prop, {
      schema: { literalBooleanSchema: true },
    });

    expect(result.schema).toEqual({
      kind: "enum",
      type: "boolean | undefined",
      schema: ["false", "true", "undefined"],
    });
  });

  it("treats comment-fragment raw prop types as lossy", () => {
    const prop: PropMeta = {
      name: "active",
      type: primitive("boolean"),
      required: false,
      hasDefault: false,
      rawType: "te. */",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("boolean | undefined");
  });

  it("resolves simple ref aliases through the type registry when that is more concrete", () => {
    const prop: PropMeta = {
      name: "type",
      type: union([ref("SingleOrMultipleType"), primitive("undefined")]),
      required: false,
      hasDefault: true,
      default: "single",
      rawType: "SingleOrMultipleType | undefined",
    };
    const typeRegistry = new Map<string, any>([
      ["SingleOrMultipleType", union([literal("single"), literal("multiple")])],
    ]);

    const result = mapPropMeta(prop, undefined, typeRegistry);

    expect(result.type).toBe('"single" | "multiple" | undefined');
  });

  it("prefers descriptor text over indexed-access raw prop types", () => {
    const prop: PropMeta = {
      name: "href",
      type: union([primitive("string"), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "NuxtLinkProps['to'] | undefined",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("string | undefined");
  });

  it("keeps NuxtLink route-location compat output exact when native resolution degrades to one object arm", () => {
    const prop: PropMeta = {
      name: "href",
      type: object([
        { name: "path", type: primitive("string"), optional: false },
        { name: "replace", type: primitive("boolean"), optional: true },
      ]),
      required: false,
      hasDefault: false,
      rawType: "NuxtLinkProps['to']",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("string | St | vt | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "string | St | vt | undefined",
      schema: [
        "string",
        "undefined",
        {
          kind: "object",
          type: "vt",
          schema: expect.objectContaining({
            path: expect.objectContaining({
              type: "string",
              required: true,
            }),
          }),
        },
        {
          kind: "object",
          type: "St",
          schema: expect.objectContaining({
            name: expect.objectContaining({
              type: "Gt",
            }),
          }),
        },
      ],
    });
  });

  it("preserves raw literal-plus-Partial union text for prefetchOn compat output", () => {
    const prop: PropMeta = {
      name: "prefetchOn",
      type: object([
        { name: "visibility", type: primitive("boolean"), optional: true },
        { name: "interaction", type: primitive("boolean"), optional: true },
      ]),
      required: false,
      hasDefault: false,
      rawType:
        "'visibility' | 'interaction' | Partial<{\n    visibility: boolean\n    interaction: boolean\n  }>",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe(
      '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }> | undefined',
    );
    expect(result.schema).toEqual({
      kind: "enum",
      type: '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }> | undefined',
      schema: [
        '"interaction"',
        '"visibility"',
        "Partial<{ visibility: boolean; interaction: boolean; }>",
        "undefined",
      ],
    });
  });

  it("expands HTMLAttributeReferrerPolicy to its literal compat enum", () => {
    const prop: PropMeta = {
      name: "referrerpolicy",
      type: union([ref("HTMLAttributeReferrerPolicy"), primitive("undefined")]),
      required: false,
      hasDefault: false,
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("HTMLAttributeReferrerPolicy | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "HTMLAttributeReferrerPolicy | undefined",
      schema: [
        '""',
        '"no-referrer-when-downgrade"',
        '"no-referrer"',
        '"origin-when-cross-origin"',
        '"origin"',
        '"same-origin"',
        '"strict-origin-when-cross-origin"',
        '"strict-origin"',
        '"unsafe-url"',
        "undefined",
      ],
    });
  });

  it("preserves function-or-array event unions in compat output", () => {
    const fn = func(
      [{ name: "event", type: ref("MouseEvent"), optional: false }],
      union([primitive("void"), ref("Promise", [primitive("void")])]),
    );
    const prop: PropMeta = {
      name: "onClick",
      type: fn,
      required: false,
      hasDefault: false,
      rawType:
        "((event: MouseEvent) => void | Promise<void>) | Array<((event: MouseEvent) => void | Promise<void>)>",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe(
      "((event: MouseEvent) => void | Promise<void>) | ((event: MouseEvent) => void | Promise<void>)[] | undefined",
    );
    expect(result.schema).toEqual({
      kind: "enum",
      type: "((event: MouseEvent) => void | Promise<void>) | ((event: MouseEvent) => void | Promise<void>)[] | undefined",
      schema: [
        "undefined",
        {
          kind: "array",
          type: "((event: MouseEvent) => void | Promise<void>)[]",
          schema: [
            {
              kind: "event",
              type: "(event: MouseEvent): void | Promise<void>",
              schema: [],
            },
          ],
        },
        {
          kind: "event",
          type: "(event: MouseEvent): void | Promise<void>",
          schema: [],
        },
      ],
    });
  });

  it("keeps bare raw ref text when the native descriptor already expanded it to literals", () => {
    const prop: PropMeta = {
      name: "dir",
      type: union([literal("ltr"), literal("rtl"), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "Direction",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("Direction | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "Direction | undefined",
      schema: ['"ltr"', '"rtl"', "undefined"],
    });
  });

  it("keeps symbolic indexed-access raw prop types when the descriptor degrades to any", () => {
    const prop: PropMeta = {
      name: "icon",
      type: union([primitive("string"), primitive("any"), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "IconProps['name']",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe('IconProps["name"] | undefined');
  });

  it("keeps symbolic ref raw prop types when the resolved descriptor is overexpanded", () => {
    const prop: PropMeta = {
      name: "value",
      type: union([ref("DateValue"), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "DateValue",
    };
    const typeRegistry = new Map<string, any>([
      [
        "DateValue",
        object(
          Array.from({ length: 64 }, (_, index) => ({
            name: `field${index}`,
            type: primitive("string"),
            optional: false,
          })),
        ),
      ],
    ]);

    const result = mapPropMeta(prop, undefined, typeRegistry);

    expect(result.type).toBe("DateValue | undefined");
  });

  it("keeps nested registry refs shallow in compat prop display and schema", () => {
    const prop: PropMeta = {
      name: "title",
      type: ref("StringOrVNode"),
      required: false,
      hasDefault: false,
      rawType: "StringOrVNode",
    };
    const typeRegistry = new Map<string, any>([
      ["StringOrVNode", union([primitive("string"), ref("VNode")])],
      [
        "VNode",
        object([
          { name: "type", type: primitive("string"), optional: false },
          { name: "component", type: ref("ComponentInternalInstance"), optional: true },
        ]),
      ],
      [
        "ComponentInternalInstance",
        object([{ name: "vnode", type: ref("VNode"), optional: false }]),
      ],
    ]);

    const result = mapPropMeta(prop, undefined, typeRegistry);

    expect(result.type).toBe("string | VNode | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "string | VNode | undefined",
      schema: [
        "string",
        {
          kind: "object",
          type: "VNode",
          schema: {},
        },
        "undefined",
      ],
    });
  });

  // @ai-generated - Guards optional raw prop normalization when generic type arguments contain `>`.
  it("keeps optional generic raw prop types without crashing", () => {
    const prop: PropMeta = {
      name: "searchInput",
      type: primitive("boolean"),
      required: false,
      hasDefault: false,
      rawType: "boolean | Omit<InputProps, 'modelValue'>",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe('boolean | Omit<InputProps, "modelValue"> | undefined');
  });

  it("normalizes chained indexed-access raw prop types without corrupting the key quotes", () => {
    const prop: PropMeta = {
      name: "color",
      type: unknown("indexedAccess"),
      required: false,
      hasDefault: false,
      rawType: "Calendar['variants']['color']",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe('Calendar["variants"]["color"] | undefined');
  });

  it("prefers descriptor text over chained indexed-access raw prop types", () => {
    const prop: PropMeta = {
      name: "activeColor",
      type: union([literal("primary"), literal("secondary"), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "Button['variants']['color']",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe('"primary" | "secondary" | undefined');
  });

  it("prefers registry-resolved object descriptors over symbolic indexed-access raw prop text", () => {
    const prop: PropMeta = {
      name: "ui",
      type: object([
        {
          name: "root",
          type: object([
            {
              name: "base",
              type: primitive("string"),
              optional: false,
            },
          ]),
          optional: true,
        },
      ]),
      required: false,
      hasDefault: false,
      rawType: "Button['slots']",
    };
    const typeRegistry = new Map<string, TypeDescriptor>([
      [
        "Button",
        object([
          {
            name: "slots",
            type: object([
              {
                name: "root",
                type: object([
                  {
                    name: "base",
                    type: primitive("string"),
                    optional: false,
                  },
                ]),
                optional: true,
              },
            ]),
            optional: false,
          },
        ]),
      ],
    ]);

    const result = mapPropMeta(prop, undefined, typeRegistry);

    expect(result.type).toBe("{ root?: { base: string; }; } | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "{ root?: { base: string; }; } | undefined",
      schema: [
        {
          kind: "object",
          schema: {
            root: {
              name: "root",
              global: false,
              required: false,
              type: "{ base: string }",
              description: "",
              tags: [],
              schema: {
                kind: "object",
                schema: {
                  base: {
                    name: "base",
                    global: false,
                    required: true,
                    type: "string",
                    description: "",
                    tags: [],
                    schema: "string",
                  },
                },
                type: "{ base: string }",
              },
            },
          },
          type: "{ root?: { base: string } }",
        },
        "undefined",
      ],
    });
  });

  it("flattens literal boolean schema members inside larger raw unions", () => {
    const prop: PropMeta = {
      name: "portal",
      type: union([primitive("boolean"), primitive("string"), ref("HTMLElement")]),
      required: false,
      hasDefault: false,
      rawType: "boolean | string | HTMLElement",
    };

    const result = mapPropMeta(prop, {
      schema: { literalBooleanSchema: true },
    });

    expect(result.schema).toEqual({
      kind: "enum",
      type: "boolean | string | HTMLElement | undefined",
      schema: [
        "false",
        "true",
        "string",
        {
          kind: "object",
          type: "HTMLElement",
          schema: {},
        },
        "undefined",
      ],
    });
  });

  it("collapses any-containing unions to compat any", () => {
    const prop: PropMeta = {
      name: "as",
      type: union([
        primitive("any"),
        object([
          { name: "root", type: primitive("any"), optional: true },
          { name: "img", type: primitive("any"), optional: true },
        ]),
        primitive("undefined"),
      ]),
      required: false,
      hasDefault: false,
      rawType: "any | { root?: any, img?: any }",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("any");
    expect(result.schema).toBe("any");
  });

  it("renders Booleanish compat enums as string values", () => {
    const prop: PropMeta = {
      name: "autofocus",
      type: union([ref("Booleanish"), primitive("undefined")]),
      required: false,
      hasDefault: false,
      rawType: "Booleanish | undefined",
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe("Booleanish | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "Booleanish | undefined",
      schema: ['"false"', '"true"', "false", "true", "undefined"],
    });
  });

  it("preserves compat ordering for branded string unions", () => {
    const prop: PropMeta = {
      name: "target",
      type: union([
        literal("_blank"),
        literal("_parent"),
        literal("_self"),
        literal("_top"),
        unknown("string & {}"),
        primitive("null"),
        primitive("undefined"),
      ]),
      required: false,
      hasDefault: false,
      rawType: '"_blank" | "_parent" | "_self" | "_top" | (string & {}) | null',
    };

    const result = mapPropMeta(prop);

    expect(result.type).toBe(
      '(string & {}) | "_blank" | "_parent" | "_self" | "_top" | null | undefined',
    );
    expect(result.schema).toMatchObject({
      kind: "enum",
      type: '(string & {}) | "_blank" | "_parent" | "_self" | "_top" | null | undefined',
      schema: [
        '"_blank"',
        '"_parent"',
        '"_self"',
        '"_top"',
        "null",
        "undefined",
        {
          kind: "object",
          type: "string & {}",
        },
      ],
    });
  });
});

describe("mapEventMeta", () => {
  it("maps an event with jsdoc", () => {
    const event: EventMeta = {
      name: "click",
      payload: unknown("unknown"),
      hasValidator: false,
      isDeclared: true,
      description: "Fired on click",
      tags: [{ name: "deprecated" }],
    };

    const result = mapEventMeta(event);

    expect(result.name).toBe("click");
    expect(result.description).toBe("Fired on click");
    expect(result.required).toBe(false);
    expect(result.tags).toEqual([{ name: "deprecated" }]);
  });

  // @ai-generated - Verifies Volar-style tuple event schemas and registry-backed payload expansion.
  it("maps tuple payload events to schema arrays and resolves registry refs", () => {
    const event: EventMeta = {
      name: "select",
      payload: tuple([ref("Item"), primitive("boolean")]),
      hasValidator: false,
      isDeclared: true,
      rawSignature: '(event: "select", item: Item, exact: boolean) => void',
    };
    const typeRegistry = new Map<string, any>([
      ["Item", object([{ name: "label", type: primitive("string"), optional: false }])],
    ]);

    const result = mapEventMeta(event, undefined, typeRegistry);

    expect(result.type).toBe("[item: Item, exact: boolean]");
    expect(result.schema).toEqual([
      {
        kind: "object",
        type: "{ label: string }",
        schema: {
          label: {
            name: "label",
            global: false,
            description: "",
            tags: [],
            required: true,
            type: "string",
            schema: "string",
          },
        },
      },
      "boolean",
    ]);
  });

  // @ai-generated - Guards tuple parsing when an event payload parameter itself is a function type.
  it("keeps later tuple parameters when raw signatures include arrow function payloads", () => {
    const event: EventMeta = {
      name: "change",
      payload: unknown("event tuple"),
      hasValidator: false,
      isDeclared: true,
      rawSignature:
        '(event: "change", transform: (value: string, count: number) => void, exact: boolean) => void',
    };

    const result = mapEventMeta(event);

    expect(result.type).toBe("[transform: (value: string, count: number) => void, exact: boolean]");
  });

  it("flattens boolean event payload schemas in literal parity mode", () => {
    const event: EventMeta = {
      name: "update:open",
      payload: primitive("boolean"),
      hasValidator: false,
      isDeclared: true,
      rawSignature: "[value: boolean]",
    };

    const result = mapEventMeta(event, {
      schema: { literalBooleanSchema: true },
    });

    expect(result.schema).toEqual(["false", "true"]);
  });
});

describe("mapSlotMeta", () => {
  it("maps a slot with bindings", () => {
    const slot: SlotMeta = {
      name: "default",
      isScoped: true,
      bindings: [{ name: "item", type: primitive("string"), rawType: "string" }],
      isRequired: true,
      description: "Main content",
    };

    const result = mapSlotMeta(slot);

    expect(result.name).toBe("default");
    expect(result.description).toBe("Main content");
    expect(result.type).toBe("{ item: string; }");
    expect(result.required).toBe(true);
  });

  it("renders detailed object bindings when raw binding text is lossy", () => {
    const slot: SlotMeta = {
      name: "leading",
      isScoped: true,
      bindings: [
        {
          name: "ui",
          rawType:
            "{ root: (props?: Record<string, any> | undefined) => string; ... 5 more ...; label: (props?: Record<string, any> | undefined) => string; }",
          type: object([
            {
              name: "root",
              type: func(
                [
                  {
                    name: "props",
                    optional: true,
                    type: union([
                      ref("Record", [primitive("string"), primitive("any")]),
                      primitive("undefined"),
                    ]),
                  },
                ],
                primitive("string"),
              ),
              optional: false,
            },
            {
              name: "label",
              type: func(
                [
                  {
                    name: "props",
                    optional: true,
                    type: union([
                      ref("Record", [primitive("string"), primitive("any")]),
                      primitive("undefined"),
                    ]),
                  },
                ],
                primitive("string"),
              ),
              optional: false,
            },
          ]),
        },
      ],
      isRequired: false,
    };

    const result = mapSlotMeta(slot);

    expect(result.type).toContain(
      "ui: { root: (props?: Record<string, any> | undefined): string; label: (props?: Record<string, any> | undefined): string; }",
    );
  });

  // @ai-generated - Verifies slot schemas are structured objects and use the shared type registry.
  it("builds structured slot schemas from bindings and resolves registry refs", () => {
    const slot: SlotMeta = {
      name: "default",
      isScoped: true,
      bindings: [{ name: "item", type: ref("Item"), rawType: "Item" }],
      isRequired: true,
    };
    const typeRegistry = new Map<string, any>([
      ["Item", object([{ name: "label", type: primitive("string"), optional: false }])],
    ]);

    const result = mapSlotMeta(slot, undefined, typeRegistry);

    expect(result.type).toBe("{ item: { label: string; }; }");
    expect(result.schema).toEqual({
      kind: "object",
      type: "{ item: { label: string } }",
      schema: {
        item: {
          name: "item",
          global: false,
          description: "",
          tags: [],
          required: true,
          type: "{ label: string }",
          schema: {
            kind: "object",
            type: "{ label: string }",
            schema: {
              label: {
                name: "label",
                global: false,
                description: "",
                tags: [],
                required: true,
                type: "string",
                schema: "string",
              },
            },
          },
        },
      },
    });
  });

  // @ai-generated - Verifies optional empty slots follow the Volar compat shape.
  it("normalizes optional empty slots to {} | undefined", () => {
    const slot: SlotMeta = {
      name: "empty",
      isScoped: false,
      bindings: [],
      isRequired: false,
    };

    const result = mapSlotMeta(slot);

    expect(result.type).toBe("{} | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "{} | undefined",
      schema: [
        "undefined",
        {
          kind: "object",
          type: "{}",
          schema: {},
        },
      ],
    });
  });
});

describe("mapExposedMeta", () => {
  it("maps exposed member", () => {
    const exposed: ExposedMeta = {
      name: "focus",
      type: unknown("() => void"),
      description: "Focus the input",
    };

    const result = mapExposedMeta(exposed);

    expect(result.name).toBe("focus");
    expect(result.description).toBe("Focus the input");
    expect(result.type).toBe("() => void");
  });

  // @ai-generated - Verifies exposed member schemas can expand shared registry refs.
  it("resolves exposed member schema through the type registry", () => {
    const exposed: ExposedMeta = {
      name: "focus",
      type: ref("Handle"),
    };
    const typeRegistry = new Map<string, any>([
      ["Handle", object([{ name: "active", type: primitive("boolean"), optional: false }])],
    ]);

    const result = mapExposedMeta(exposed, undefined, typeRegistry);

    expect(result.schema).toEqual({
      kind: "object",
      type: "{ active: boolean }",
      schema: {
        active: {
          name: "active",
          global: false,
          description: "",
          tags: [],
          required: true,
          type: "boolean",
          schema: "boolean",
        },
      },
    });
  });
});

describe("mapComponentMeta", () => {
  it("produces VolarComponentMeta shape with _verter", () => {
    const meta = {
      filePath: "test.vue",
      componentName: "Test",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: primitive("string"),
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
    };

    const result = mapComponentMeta(meta);

    expect(result.type).toBe(0);
    expect(result.props).toHaveLength(1);
    expect(result.props[0].name).toBe("label");
    expect(result.events).toHaveLength(0);
    expect(result.slots).toHaveLength(0);
    expect(result.exposed).toHaveLength(0);
    expect(result._verter).toBe(meta);

    // Negative: compat shape should NOT have Verter-only fields at top level
    expect("filePath" in result).toBe(false);
    expect("componentName" in result).toBe(false);
    expect("models" in result).toBe(false);
    expect("flags" in result).toBe(false);
  });

  it("filters known VNode/internal slot names from compat output", () => {
    const meta = {
      filePath: "test.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [
        { name: "default", isScoped: false, bindings: [], isRequired: false },
        { name: "type", isScoped: false, bindings: [], isRequired: false },
        { name: "props", isScoped: false, bindings: [], isRequired: false },
        { name: "appContext", isScoped: false, bindings: [], isRequired: false },
        { name: "targetStart", isScoped: false, bindings: [], isRequired: false },
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
    };

    const result = mapComponentMeta(meta);

    expect(result.slots.map((slot) => slot.name)).toEqual(["default"]);
  });

  it("keeps compat exposed on the analysis surface when a public-instance sidecar exists", () => {
    const meta = {
      filePath: "test.vue",
      componentName: "Test",
      optionsApi: false,
      props: [],
      events: [],
      slots: [],
      models: [],
      exposed: [
        {
          name: "analysisOnly",
          type: primitive("number"),
        },
      ],
      publicInstance: {
        completeness: "partial",
        members: [
          {
            name: "$slots",
            kind: "slotContainer",
            type: object([]),
          },
          {
            name: "label",
            kind: "prop",
            type: primitive("string"),
          },
          {
            name: "focus",
            kind: "exposed",
            type: primitive("number"),
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
    };

    const result = mapComponentMeta(meta as any);

    expect(result.exposed.map((member) => member.name)).toEqual(["analysisOnly"]);
  });
});

// ── Checker integration tests ───────────────────────────────────────

describe("ComponentMetaChecker", () => {
  it("uses the declared native component-meta query instead of rebuilding from analysis snapshots", async () => {
    const getDeclaredComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const getResolvedComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const getComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const session = {
      closed: false,
      engine: { state: "active" as const },
      upsert() {},
      delete() {},
      getDeclaredComponentMeta,
      getResolvedComponentMeta,
      getComponentMeta,
      getProvenance() {
        return "{}";
      },
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "/tmp",
      {},
      session as any,
    );

    checker.updateFile(
      "Single.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );
    const meta = await checker.getComponentMeta("Single.vue");

    expect(meta.props.some((prop) => prop.name === "label")).toBe(true);
    expect(getDeclaredComponentMeta).toHaveBeenCalledTimes(1);
    expect(getDeclaredComponentMeta).toHaveBeenCalledWith(resolvePath("/tmp", "Single.vue"));
    expect(getResolvedComponentMeta).not.toHaveBeenCalled();
    expect(getComponentMeta).not.toHaveBeenCalled();
  });

  it("createCheckerByJson reuses one pooled engine across include differences in selective-loading mode", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `checker-pool-include-${nextProjectRootId++}`,
    );

    const checkerA = await createCheckerByJson(projectRoot, {
      include: ["src/A.vue"],
      compilerOptions: { baseUrl: "." },
    });
    const checkerB = await createCheckerByJson(projectRoot, {
      include: ["src/B.vue"],
      compilerOptions: { baseUrl: "." },
    });

    expect(runtime.engineCount).toBe(1);
    expect(runtime.diagnostics.enginesCreated).toBe(1);
    expect(runtime.diagnostics.enginesReused).toBe(1);

    checkerA.close();
    checkerB.close();
    shutdownMetaRuntime();
  });

  it("createCheckerByJson uses pooled runtime leases instead of dedicated engines", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-pooled-json-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createCheckerByJson(projectRoot, {
      include: ["src/**/*.vue"],
      compilerOptions: { baseUrl: "." },
    });

    expect(runtime.engineCount).toBe(1);
    expect(runtime.sessionCount).toBe(1);

    const engine = (checker as any)._session.engine;
    expect(engine.state).toBe("active");

    checker.close();

    expect(runtime.engineCount).toBe(1);
    expect(runtime.sessionCount).toBe(0);
    expect(engine.state).toBe("active");
    shutdownMetaRuntime();
  });

  it("createCheckerByJson can opt into dedicated runtime mode for benchmark isolation", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-dedicated-json-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createCheckerByJson(
      projectRoot,
      {
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      },
      {
        runtimeMode: "dedicated",
      },
    );

    const checkerRuntime = (checker as any)._runtime;
    const engine = (checker as any)._session.engine;

    expect(runtime.engineCount).toBe(0);
    expect(checkerRuntime).toBeTruthy();
    expect(checkerRuntime).not.toBe(runtime);
    expect(engine.state).toBe("active");

    checker.close();

    expect(engine.state).toBe("closed");
    expect(runtime.engineCount).toBe(0);
    shutdownMetaRuntime();
  });

  it("createCheckerByJson resolves overlay-only imported helpers even when the root does not exist yet", { timeout: 15_000 }, async () => {
    shutdownMetaRuntime();
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-checker-missing-root-${nextProjectRootId++}`,
    );
    rmSync(projectRoot, { recursive: true, force: true });

    const checker = await createCheckerByJson(
      projectRoot,
      {},
      {
        runtimeMode: "dedicated",
      },
    );

    checker.updateFile(
      "types.ts",
      `type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: ClassNameValue
}

type ComponentUI<T extends { slots?: Record<string, any> }> = {
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
  appConfig?: A
}`,
    );
    checker.updateFile(
      "theme.ts",
      `export default {
  variants: {
    color: { primary: '', secondary: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const`,
    );
    checker.updateFile(
      "Button.vue",
      `<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'
import type { VNode } from 'vue'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  leading?(props: { ui: Button['ui'] }): VNode[]
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>`,
    );

    await settleNativeProject();

    const meta = await checker.getComponentMeta("Button.vue");
    const color = meta.props.find((prop) => prop.name === "color");
    const ui = meta.props.find((prop) => prop.name === "ui");
    const leading = meta.slots.find((slot) => slot.name === "leading");

    expect(color).toBeDefined();
    expect(color!.type).toContain("primary");
    expect(color!.type).toContain("secondary");
    expect(color!.type).not.toContain('Button["variants"]["color"]');
    expect(ui).toBeDefined();
    expect(ui!.type).toBe("{ base?: ClassNameValue; label?: ClassNameValue; } | undefined");
    expect(ui!.schema).toEqual({
      kind: "enum",
      type: "{ base?: ClassNameValue; label?: ClassNameValue; } | undefined",
      schema: ["{ base?: ClassNameValue; label?: ClassNameValue; }", "undefined"],
    });
    expect(leading).toBeDefined();
    expect(leading!.type).toContain("ui: {");
    expect(leading!.type).toContain("base: (props?: Record<string, any> | undefined) => string");
    expect(leading!.type).toContain("label: (props?: Record<string, any> | undefined) => string");

    checker.close();
  });

  it("dedicated runtime mode does not reuse benchmark-created engines after dispose", async () => {
    shutdownMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-dedicated-reopen-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checkerA = await createCheckerByJson(
      projectRoot,
      {
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      },
      {
        runtimeMode: "dedicated",
      },
    );
    const runtimeA = (checkerA as any)._runtime;
    const engineA = (checkerA as any)._session.engine;
    checkerA.close();

    const checkerB = await createCheckerByJson(
      projectRoot,
      {
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      },
      {
        runtimeMode: "dedicated",
      },
    );
    const runtimeB = (checkerB as any)._runtime;
    const engineB = (checkerB as any)._session.engine;

    expect(runtimeA).not.toBe(runtimeB);
    expect(engineA).not.toBe(engineB);
    expect(engineA.state).toBe("closed");
    expect(engineB.state).toBe("active");

    checkerB.close();
    shutdownMetaRuntime();
  });

  it("createChecker reuses one pooled engine for repeated tsconfig opens", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-pooled-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      }),
      "utf8",
    );
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checkerA = await createChecker(resolve(projectRoot, "tsconfig.json"));
    const checkerB = await createChecker(resolve(projectRoot, "tsconfig.json"));

    expect(runtime.engineCount).toBe(1);
    expect(runtime.diagnostics.enginesCreated).toBe(1);
    expect(runtime.diagnostics.enginesReused).toBe(1);

    checkerA.close();
    checkerB.close();
    shutdownMetaRuntime();
  });

  it("createChecker and openComponentMetaSession share one pooled engine for the same tsconfig", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-project-shared-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      }),
      "utf8",
    );
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createChecker(resolve(projectRoot, "tsconfig.json"));
    const project = await openComponentMetaSession({
      root: projectRoot,
      tsconfig: resolve(projectRoot, "tsconfig.json"),
    });

    expect(runtime.engineCount).toBe(1);
    expect(runtime.diagnostics.enginesCreated).toBe(1);
    expect(runtime.diagnostics.enginesReused).toBe(1);

    checker.close();
    project.close();
    shutdownMetaRuntime();
  });

  // @ai-generated - Verifies the public drop-in compat entrypoint creates its own workspace.
  it("createChecker accepts a tsconfig path without an injected workspace", async () => {
    shutdownMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-tsconfig-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      }),
      "utf8",
    );
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createChecker(resolve(projectRoot, "tsconfig.json"));
    const meta = await checker.getComponentMeta("./src/App.vue");

    expect(meta.props.some((prop) => prop.name === "label")).toBe(true);

    checker.close();
    shutdownMetaRuntime();
  });

  it("promotes lazy workspace files into the shared native project", async () => {
    const canonicalId = resolvePath("/tmp", "Lazy.vue");
    const ensureBaseFile = vi.fn(() => true);
    const getDeclaredComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const getResolvedComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const getComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const workspace = {
      readFile: vi.fn(async () => {
        throw new Error("JS workspace read should not be used for lazy native loading");
      }),
      fileExists: vi.fn(async () => true),
      isDir: vi.fn(async () => false),
      readDir: vi.fn(async () => []),
      walk: vi.fn(async () => []),
      configureProjects: vi.fn(),
    };
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "/tmp",
      {},
      {
        closed: false,
        engine: { state: "active" as const },
        upsert: vi.fn(),
        delete: vi.fn(),
        ensureBaseFile,
        getDeclaredComponentMeta,
        getResolvedComponentMeta,
        getComponentMeta,
        getProvenance() {
          return "{}";
        },
        getEffectiveSource(id: string) {
          if (id === canonicalId && ensureBaseFile.mock.calls.length > 0) {
            return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
          }
          return undefined;
        },
        hasFile() {
          return false;
        },
        trackedFileIds() {
          return [];
        },
        close() {},
      } as any,
      workspace as any,
    );

    const meta = await checker.getComponentMeta("Lazy.vue");

    expect(ensureBaseFile).toHaveBeenCalledWith(canonicalId);
    expect(workspace.readFile).not.toHaveBeenCalled();
    expect(getDeclaredComponentMeta).toHaveBeenCalledWith(canonicalId);
    expect(getDeclaredComponentMeta).toHaveBeenCalledTimes(1);
    expect(getResolvedComponentMeta).not.toHaveBeenCalled();
    expect(getComponentMeta).not.toHaveBeenCalled();
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
  });

  it("getComponentMeta returns Volar-shaped output", async () => {
    const checker = await createRuntimeChecker("checker-volar-shape");

    const source = `<script setup lang="ts">
/** The label text */
defineProps<{
  /** Display label */
  label: string
  /** @deprecated use label instead */
  title?: string
}>()

defineEmits<{
  /** Fired on click */
  click: []
}>()
</script>
<template><div><slot /></div></template>`;

    checker.updateFile("Test.vue", source);
    const meta = await checker.getComponentMeta("Test.vue");

    // Shape checks
    expect(meta.type).toBe(0);
    expect(Array.isArray(meta.props)).toBe(true);
    expect(Array.isArray(meta.events)).toBe(true);
    expect(Array.isArray(meta.slots)).toBe(true);
    expect(Array.isArray(meta.exposed)).toBe(true);

    // Props
    expect(meta.props.length).toBeGreaterThanOrEqual(1);
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.description).toBe("Display label");
    expect(labelProp!.type).toBe("string");
    expect(labelProp!.required).toBe(true);
    expect(labelProp!.tags).toEqual([]);
    expect(typeof labelProp!.schema).toBeDefined();

    // Title prop with @deprecated tag
    const titleProp = meta.props.find((p) => p.name === "title");
    if (titleProp) {
      expect(titleProp.tags.length).toBeGreaterThanOrEqual(1);
      expect(titleProp.tags[0].name).toBe("deprecated");
    }

    // Events
    const clickEvent = meta.events.find((e) => e.name === "click");
    if (clickEvent) {
      expect(clickEvent.description).toBe("Fired on click");
    }

    // _verter field has full metadata
    expect(meta._verter).toBeDefined();
    expect(meta._verter!.filePath).toBeDefined();
    expect(meta._verter!.componentName).toBeDefined();
  });

  it("uses the declared native query by default when a compat query exists", async () => {
    const canonicalId = "Fast.vue";
    const getDeclaredComponentMeta = vi.fn(() => nativeMetaPayload("/project/Fast.vue"));
    const getResolvedComponentMeta = vi.fn(() => nativeMetaPayload("/project/Fast.vue"));
    const getComponentMeta = vi.fn(() => nativeMetaPayload("/project/Fast.vue"));
    const checker = new ComponentMetaChecker(
      {} as any,
      "/project",
      {},
      {
        engine: { state: "active" as const },
        upsert() {},
        delete() {},
        getDeclaredComponentMeta,
        getResolvedComponentMeta,
        getComponentMeta,
        getProvenance() {
          return "{}";
        },
        close() {},
        getEffectiveSource() {
          return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
        },
        hasFile() {
          return true;
        },
        trackedFileIds() {
          return [canonicalId];
        },
        get overlayGeneration() {
          return 0;
        },
        get closed() {
          return false;
        },
      } as any,
      undefined,
      {
        closeSession() {},
      } as any,
    );

    const meta = await checker.getComponentMeta(canonicalId);

    expect(getDeclaredComponentMeta).toHaveBeenCalledWith(resolvePath("/project", canonicalId));
    expect(getResolvedComponentMeta).not.toHaveBeenCalled();
    expect(getComponentMeta).not.toHaveBeenCalled();
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
  });

  it("getExportNames returns ['default'] for SFC", async () => {
    const checker = await createRuntimeChecker("checker-export-names");
    expect(await checker.getExportNames("Test.vue")).toEqual(["default"]);
  });

  it("updateFile is reflected in next getComponentMeta", async () => {
    const checker = await createRuntimeChecker("checker-update-file");

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>`,
    );
    let meta = await checker.getComponentMeta("Test.vue");
    expect(meta.props.some((p) => p.name === "a")).toBe(true);

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ b: number }>()</script><template><div /></template>`,
    );
    meta = await checker.getComponentMeta("Test.vue");
    expect(meta.props.some((p) => p.name === "b")).toBe(true);
    expect(meta.props.some((p) => p.name === "a")).toBe(false);
  });

  it("restoreBaseFile clears a temporary overlay and reveals the disk-backed file", async () => {
    const projectRoot = resolve(tmpdir(), `checker-restore-file-${nextProjectRootId++}`);
    mkdirSync(projectRoot, { recursive: true });
    writeFileSync(
      resolve(projectRoot, "Test.vue"),
      `<script setup lang="ts">defineProps<{ base: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createCheckerByJson(projectRoot, {});
    try {
      checker.updateFile(
        "Test.vue",
        `<script setup lang="ts">defineProps<{ temp: number }>()</script><template><div /></template>`,
      );
      let meta = await checker.getComponentMeta("Test.vue");
      expect(meta.props.some((p) => p.name === "temp")).toBe(true);
      expect(meta.props.some((p) => p.name === "base")).toBe(false);

      checker.restoreBaseFile("Test.vue");
      meta = await checker.getComponentMeta("Test.vue");
      expect(meta.props.some((p) => p.name === "base")).toBe(true);
      expect(meta.props.some((p) => p.name === "temp")).toBe(false);
    } finally {
      checker.close();
      rmSync(projectRoot, { recursive: true, force: true });
    }
  });

  it("deleteFile clears metadata", async () => {
    const checker = await createRuntimeChecker("checker-delete-file");

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>`,
    );
    checker.deleteFile("Test.vue");
    const meta = await checker.getComponentMeta("Test.vue");
    expect(meta.props).toHaveLength(0);
  });

  it("deleteFile does not lazily rehydrate a tombstoned base file", async () => {
    let workspaceReads = 0;
    const workspace = {
      readFile: async () => {
        workspaceReads++;
        return `<script setup lang="ts">defineProps<{ a: string }>()</script>`;
      },
      fileExists: async () => true,
      isDir: async () => false,
      readDir: async () => [],
      walk: async () => [],
      configureProjects() {},
    };
    const session = {
      closed: false,
      engine: { state: "active" as const },
      upsert() {},
      delete() {},
      getDeclaredComponentMeta() {
        return null;
      },
      getComponentMeta() {
        return null;
      },
      getProvenance() {
        return "{}";
      },
      getEffectiveSource() {
        return undefined;
      },
      hasFile() {
        return false;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };
    const checker = new ComponentMetaChecker(
      {
        upsert() {},
        remove() {},
      },
      "/tmp",
      {},
      session as any,
      workspace,
      { closeSession() {} } as any,
    );

    checker.deleteFile("Base.vue");
    const meta = await checker.getComponentMeta("Base.vue");

    expect(meta.props).toHaveLength(0);
    expect(workspaceReads).toBe(0);
  });

  it("getProgram throws", () => {
    const checker = new ComponentMetaChecker(
      {
        upsert() {},
      },
      "/tmp",
      {},
    );
    expect(() => checker.getProgram()).toThrow();
  });

  it("runtime defineProps preserves JSDoc descriptions and tags", async () => {
    const checker = await createRuntimeChecker("checker-runtime-props");

    const source = `<script setup lang="ts">
defineProps({
  /** The display label */
  label: String,
  /** Size variant
   * @default 'md'
   */
  size: { type: String, default: 'md' },
  noDoc: Number,
})
</script>
<template><div /></template>`;

    checker.updateFile("Runtime.vue", source);
    const meta = await checker.getComponentMeta("Runtime.vue");

    // Positive: label has JSDoc description
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.description).toBe("The display label");
    expect(labelProp!.tags).toEqual([]);

    // Positive: size has JSDoc description and @default tag
    const sizeProp = meta.props.find((p) => p.name === "size");
    expect(sizeProp).toBeDefined();
    expect(sizeProp!.description).toBe("Size variant");
    expect(sizeProp!.tags.length).toBeGreaterThanOrEqual(1);
    expect(sizeProp!.tags[0].name).toBe("default");

    // Negative: noDoc has empty description (compat maps null → "")
    const noDocProp = meta.props.find((p) => p.name === "noDoc");
    expect(noDocProp).toBeDefined();
    expect(noDocProp!.description).toBe("");
    expect(noDocProp!.tags).toEqual([]);
  });

  it("enum schema uses array format (Volar parity)", async () => {
    const checker = await createRuntimeChecker("checker-enum-schema");

    const source = `<script setup lang="ts">
defineProps<{
  color?: 'red' | 'blue'
}>()
</script>
<template><div /></template>`;

    checker.updateFile("Enum.vue", source);
    const meta = await checker.getComponentMeta("Enum.vue");

    const colorProp = meta.props.find((p) => p.name === "color");
    expect(colorProp).toBeDefined();
    const schema = colorProp!.schema;
    // Should be enum with array schema, not numeric-keyed object
    expect(typeof schema).not.toBe("string");
    if (typeof schema !== "string") {
      expect(schema.kind).toBe("enum");
      expect(Array.isArray(schema.schema)).toBe(true);
    }
  });

  it("resolves imported type interfaces from dependency .ts files", async () => {
    // Use resolve() to get consistent absolute paths on all platforms
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-crossfile-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    // Upsert the .ts dependency as non-SFC using resolved path
    checker.updateFile("types.ts", "export interface ButtonProps { label: string; size?: number }");

    // Upsert the .vue file that imports from the dependency
    checker.updateFile(
      "Button.vue",
      `<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Button.vue");

    // Positive: should resolve props from the imported interface
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.type).toContain("string");

    const sizeProp = meta.props.find((p) => p.name === "size");
    expect(sizeProp).toBeDefined();

    // Negative: should not have the interface name as a prop
    expect(meta.props.some((p) => p.name === "ButtonProps")).toBe(false);
  });

  // ReturnType<typeof fn> resolved by native evaluator via body inference.
  it("keeps ReturnType utility props as opaque type string without JS resolver", async () => {
    const checker = await createRuntimeChecker("checker-return-type");

    checker.updateFile(
      "ReturnType.vue",
      `<script setup lang="ts">
function createConfig() {
  return { theme: 'dark' as string, debug: false }
}

defineProps<{
  config: ReturnType<typeof createConfig>
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("ReturnType.vue");
    const configProp = meta.props.find((p) => p.name === "config");

    expect(configProp).toBeDefined();
    // Native payload does not expand utility types like ReturnType<> into
    // object schemas — the schema is the opaque type-string ref.
    expect(configProp!.type).toContain("ReturnType");
  });

  // Pick/Omit stay as opaque type strings without JS resolver expansion.
  it("keeps Pick and Omit utility props as opaque type strings without JS resolver", async () => {
    const checker = await createRuntimeChecker("checker-pick-omit");

    checker.updateFile(
      "PickOmit.vue",
      `<script setup lang="ts">
interface FullUser {
  id: number
  name: string
  email: string
  password: string
}

defineProps<{
  display: Pick<FullUser, 'id' | 'name'>
  safe: Omit<FullUser, 'password'>
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("PickOmit.vue");
    const displayProp = meta.props.find((p) => p.name === "display");
    const safeProp = meta.props.find((p) => p.name === "safe");

    expect(displayProp).toBeDefined();
    expect(safeProp).toBeDefined();
    // Native payload does not expand Pick/Omit utilities into narrowed
    // object schemas — the schemas remain as opaque type-string refs.
    expect(displayProp!.type).toContain("Pick");
    expect(safeProp!.type).toContain("Omit");
  });

  it("keeps imported Pick utility props as opaque type strings without JS resolver", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-utilities-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "types.ts",
      `export interface ImportedUser {
  id: number
  name: string
  password: string
}`,
    );

    checker.updateFile(
      "ImportedPick.vue",
      `<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: Pick<ImportedUser, 'id' | 'name'>
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("ImportedPick.vue");
    const userProp = meta.props.find((p) => p.name === "user");

    expect(userProp).toBeDefined();
    // Native payload does not expand Pick<ImportedType, ...> into narrowed
    // object schemas — the schema remains as an opaque type-string ref.
    expect(userProp!.type).toContain("Pick");

    checker.close();
  });

  it("preserves inherited imported props through Omit chains and keeps explicit class", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-inherited-props-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "types.ts",
      `export interface LinkProps {
  as?: string
  class?: any
  href?: string
  target?: string
  active?: boolean
}

export type LinkPropsKeys = 'href' | 'target' | 'active'

export interface ButtonProps extends Omit<LinkProps, 'href'> {
  label?: string
  color?: string
  variant?: string
  ui?: object
}`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: 'primary'
  variant?: 'solid'
  side?: 'left' | 'right'
}

defineProps<Props>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const propNames = meta.props.map((prop) => prop.name);

    expect(propNames).toEqual(
      expect.arrayContaining(["as", "class", "label", "ui", "color", "variant", "side"]),
    );
    expect(propNames).not.toEqual(expect.arrayContaining(["href", "target", "active"]));

    checker.close();
  });

  it("preserves inherited imported emits through Omit chains", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-inherited-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "events.ts",
      `export interface MenuContentImplEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'pointerDownOutside', event: PointerEvent): void
  (e: 'focusOutside', event: FocusEvent): void
  (e: 'interactOutside', event: Event): void
  (e: 'openAutoFocus'): void
  (e: 'closeAutoFocus'): void
  (e: 'entryFocus'): void
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>

export interface ContextMenuContentEmits extends MenuContentEmits {}`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { ContextMenuContentEmits } from './events'

defineEmits<ContextMenuContentEmits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const events = Object.fromEntries(meta.events.map((event) => [event.name, event.type]));

    expect(Object.keys(events)).toEqual(
      expect.arrayContaining([
        "escapeKeyDown",
        "pointerDownOutside",
        "focusOutside",
        "interactOutside",
        "closeAutoFocus",
      ]),
    );
    expect(Object.keys(events)).not.toEqual(
      expect.arrayContaining(["openAutoFocus", "entryFocus"]),
    );
    expect(events.escapeKeyDown).toContain("Event");

    checker.close();
  });

  it("preserves locally declared emits without cross-file inherited call-signature expansion", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-mixed-inherited-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "events.ts",
      `export interface MenuContentEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'focusOutside', event: FocusEvent): void
}`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { MenuContentEmits } from './events'

interface Emits extends MenuContentEmits {
  (e: 'closeAutoFocus'): void
}

defineEmits<Emits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const eventNames = meta.events.map((event) => event.name);

    // Native payload resolves only locally-declared call-signature emits.
    // Cross-file inherited call-signature members from imported interfaces
    // are not expanded without a JS resolver.
    expect(eventNames).toContain("closeAutoFocus");

    checker.close();
  });

  it("preserves local tuple-property emits without cross-file inherited call-signature expansion", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-mixed-inherited-tuple-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "events.ts",
      `export interface MenuContentEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'focusOutside', event: FocusEvent): void
}`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { MenuContentEmits } from './events'

interface Emits extends MenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const events = Object.fromEntries(meta.events.map((event) => [event.name, event.type]));

    // Native payload resolves only locally-declared tuple-property emits.
    // Cross-file inherited call-signature members are not expanded without a JS resolver.
    expect(Object.keys(events)).toContain("update:searchTerm");
    expect(events["update:searchTerm"]).toContain("value: string");

    checker.close();
  });

  it("preserves local tuple-property emits without cross-file alias-chain call-signature expansion", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-mixed-inherited-alias-tuple-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "events.ts",
      `export interface MenuContentImplEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'pointerDownOutside', event: PointerEvent): void
  (e: 'focusOutside', event: FocusEvent): void
  (e: 'interactOutside', event: Event): void
  (e: 'openAutoFocus'): void
  (e: 'closeAutoFocus'): void
  (e: 'entryFocus'): void
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { MenuContentEmits } from './events'

interface Emits extends MenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const eventNames = meta.events.map((event) => event.name);

    // Native payload resolves only locally-declared tuple-property emits.
    // Cross-file inherited Omit-chained call-signature members are not
    // expanded without a JS resolver.
    expect(eventNames).toContain("update:searchTerm");

    checker.close();
  });

  it("preserves local tuple-property emits without cross-file alias-chain expansion through import aliases", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-aliased-inherited-alias-tuple-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "events.ts",
      `export interface MenuContentImplEmits {
  escapeKeyDown: [event: KeyboardEvent]
  pointerDownOutside: [event: PointerEvent]
  focusOutside: [event: FocusEvent]
  interactOutside: [event: Event]
  openAutoFocus: [event: Event]
  closeAutoFocus: [event: Event]
  entryFocus: [event: Event]
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { MenuContentEmits as LocalMenuContentEmits } from './events'

interface Emits extends LocalMenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const eventNames = meta.events.map((event) => event.name);

    // Native payload resolves only locally-declared tuple-property emits.
    // Cross-file inherited Omit-chained tuple-property members through
    // local import aliases are not expanded without a JS resolver.
    expect(eventNames).toContain("update:searchTerm");

    checker.close();
  });

  it("materializes imported mapped slots and does not synthesize default from dynamic branches", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-mapped-slots-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "slots.ts",
      `export interface PricingPlan {
  id: string
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any
  title(props: { planId: string }): any
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey]

export type PricingPlansSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in keyof PricingPlanSlots]?: ExtendSlotWithPlan<TPlan, K>
} & {
  default?(props?: {}): any
}

export type TableSlots = {
  expanded?(props: { row: string }): any
  empty?(props?: {}): any
} & Record<string, (props: any) => any>`,
    );

    checker.updateFile(
      "PricingPlans.vue",
      `<script setup lang="ts">
import type { PricingPlansSlots } from './slots'

defineSlots<PricingPlansSlots<{ id: string; tier: 'pro' }>>()
</script>
<template><div /></template>`,
    );

    checker.updateFile(
      "Table.vue",
      `<script setup lang="ts">
import type { TableSlots } from './slots'

defineSlots<TableSlots>()
</script>
<template><div /></template>`,
    );

    const pricingPlans = await checker.getComponentMeta("PricingPlans.vue");
    expect(pricingPlans.slots.map((slot) => slot.name)).toEqual(
      expect.arrayContaining(["badge", "title", "default"]),
    );
    // TODO(follow-up): slot bindings require full type-solver evaluation of
    // the conditional type ExtendSlotWithPlan<TPlan, K> which the source-text
    // resolver cannot handle.  Once the session solver is used for slot
    // binding materialization, assert plan/planId are present in badgeType.
    const badgeSlot = pricingPlans.slots.find((slot) => slot.name === "badge");
    expect(badgeSlot).toBeDefined();

    const table = await checker.getComponentMeta("Table.vue");
    expect(table.slots.map((slot) => slot.name)).toEqual(
      expect.arrayContaining(["expanded", "empty"]),
    );
    expect(table.slots.map((slot) => slot.name)).not.toContain("default");

    checker.close();
  });

  it("does not synthesize default from dynamic template slot names", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-dynamic-template-slot-names-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
const sectionSlot = 'section-title'
</script>
<template>
  <div>
    <slot :name="sectionSlot" />
    <slot name="caption" />
  </div>
</template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const slotNames = meta.slots.map((slot) => slot.name);

    expect(slotNames).toContain("caption");
    expect(slotNames).not.toContain("default");

    checker.close();
  });

  it("resolves namespace-qualified imported props", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-namespace-props-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "types.ts",
      `export interface BaseProps {
  a?: string
  b?: number
}

export interface Props extends BaseProps {
  c?: boolean
}`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type * as Types from './types'

defineProps<Types.Props>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const propNames = meta.props.map((prop) => prop.name);

    expect(propNames).toEqual(expect.arrayContaining(["a", "b", "c"]));
    expect(propNames).not.toContain("default");

    checker.close();
  });

  it("materializes mapped tuple emits from type-based defineEmits", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-mapped-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
type Emits = {
  [K in 'open' | 'close']?: []
}

defineEmits<Emits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const eventNames = meta.events.map((event) => event.name);

    expect(eventNames).toEqual(expect.arrayContaining(["open", "close"]));
    expect(eventNames).not.toContain("default");

    checker.close();
  });

  it("materializes imported mapped tuple emits from type-based defineEmits", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-mapped-emits-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "events.ts",
      `export type Emits = {
  [K in 'open' | 'close']?: []
}`,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">
import type { Emits } from './events'

defineEmits<Emits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("App.vue");
    const eventNames = meta.events.map((event) => event.name);

    expect(eventNames).toEqual(expect.arrayContaining(["open", "close"]));
    expect(eventNames).not.toContain("default");

    checker.close();
  });

  it("preserves index signature text inside intersection schemas", async () => {
    const checker = await createRuntimeChecker("checker-index-signature");

    checker.updateFile(
      "Typed.vue",
      `<script setup lang="ts">
defineProps<{
  partialImage: string | (Partial<HTMLImageElement> & { [key: string]: any })
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Typed.vue");
    const partialImage = meta.props.find((p) => p.name === "partialImage");

    expect(partialImage).toBeDefined();
    expect(typeof partialImage!.schema).not.toBe("string");
    if (typeof partialImage!.schema === "string") return;

    expect(partialImage!.schema.kind).toBe("enum");
    const objectArm = (partialImage!.schema.schema as any[]).find(
      (entry) =>
        typeof entry !== "string" && entry?.kind === "object" && Array.isArray(entry?.schema),
    );
    expect(objectArm).toBeDefined();
    expect(objectArm.schema).toEqual([
      expect.objectContaining({
        kind: "object",
        type: "Partial<HTMLImageElement>",
      }),
      expect.objectContaining({
        kind: "object",
        type: "{ [key: string]: any; }",
      }),
    ]);
  });

  it("Options API props preserve JSDoc descriptions and tags", async () => {
    const checker = await createRuntimeChecker("checker-options-api");

    const source = `<script>
import { defineComponent } from 'vue'
export default defineComponent({
  props: {
    /** The display label */
    label: String,
    /** Size variant
     * @default 'md'
     */
    size: { type: String, default: 'md' },
    noDoc: Number,
  }
})
</script>
<template><div /></template>`;

    checker.updateFile("Options.vue", source);
    const meta = await checker.getComponentMeta("Options.vue");

    // Positive: label has JSDoc description
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.description).toBe("The display label");
    expect(labelProp!.tags).toEqual([]);

    // Positive: size has JSDoc description and @default tag
    const sizeProp = meta.props.find((p) => p.name === "size");
    expect(sizeProp).toBeDefined();
    expect(sizeProp!.description).toBe("Size variant");
    expect(sizeProp!.tags.length).toBeGreaterThanOrEqual(1);
    expect(sizeProp!.tags[0].name).toBe("default");

    // Negative: noDoc has empty description (compat maps null → "")
    const noDocProp = meta.props.find((p) => p.name === "noDoc");
    expect(noDocProp).toBeDefined();
    expect(noDocProp!.description).toBe("");
  });

  it("keeps imported component-prop helpers and Numberish aliases exact", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-component-props-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "types.ts",
      `export * from './Chip.vue'
export * from './Avatar.vue'

export type Numberish = number | string`,
    );

    checker.updateFile(
      "Chip.vue",
      `<script lang="ts">
export interface ChipProps {
  /**
   * The element or component this component should render as.
   * @defaultValue 'div'
   */
  as?: any
  text?: string | number
}
</script>
<script setup lang="ts">
defineProps<ChipProps>()
</script>
<template><div /></template>`,
    );

    checker.updateFile(
      "Avatar.vue",
      `<script lang="ts">
import type { ChipProps, Numberish } from './types'

export interface AvatarProps {
  /**
   * The element or component this component should render as.
   * @defaultValue 'span'
   */
  as?: any | { root?: any, img?: any }
  chip?: boolean | ChipProps
  height?: Numberish
  width?: Numberish
}
</script>
<script setup lang="ts">
defineProps<AvatarProps>()
</script>
<template><div /></template>`,
    );

    const avatarMeta = await checker.getComponentMeta("Avatar.vue");
    const asProp = avatarMeta.props.find((prop) => prop.name === "as");
    const chipProp = avatarMeta.props.find((prop) => prop.name === "chip");
    const heightProp = avatarMeta.props.find((prop) => prop.name === "height");

    expect(asProp).toBeDefined();
    expect(asProp!.type).toBe("any");
    expect(asProp!.schema).toBe("any");

    expect(chipProp).toBeDefined();
    expect(typeof chipProp!.schema).not.toBe("string");
    if (typeof chipProp!.schema !== "string") {
      // Without JS resolver, boolean stays as "boolean" (not split to
      // "false"/"true"), and ChipProps is an opaque ref with empty schema.
      expect(chipProp!.schema).toEqual({
        kind: "enum",
        type: "boolean | ChipProps | undefined",
        schema: [
          "boolean",
          {
            kind: "object",
            type: "ChipProps",
            schema: {},
          },
          "undefined",
        ],
      });
    }

    expect(heightProp).toBeDefined();
    expect(heightProp!.type).toBe("Numberish | undefined");
    expect(heightProp!.schema).toEqual({
      kind: "enum",
      type: "Numberish | undefined",
      schema: ["number", "string", "undefined"],
    });

    checker.close();
  });

  it("keeps nested referenced component props compact and rewrites defaulted enum schema types", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-nested-component-props-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "types.ts",
      `export * from './Chip.vue'
export * from './Avatar.vue'`,
    );

    checker.updateFile(
      "Chip.vue",
      `<script lang="ts">
type Chip = {
  variants: {
    size: 'sm' | 'md'
  }
}

export interface ChipProps {
  /**
   * @defaultValue 'div'
   */
  as?: any
  text?: string | number
  /**
   * @defaultValue 'md'
   */
  size?: Chip['variants']['size']
}
</script>
<script setup lang="ts">
defineProps<ChipProps>()
defineModel<boolean>('show', { default: true })
</script>
<template><div /></template>`,
    );

    checker.updateFile(
      "Avatar.vue",
      `<script lang="ts">
import type { ChipProps } from './types'

export interface AvatarProps {
  chip?: boolean | ChipProps
}
</script>
<script setup lang="ts">
defineProps<AvatarProps>()
</script>
<template><div /></template>`,
    );

    checker.updateFile(
      "Alert.vue",
      `<script lang="ts">
import type { AvatarProps } from './Avatar.vue'

export interface AlertProps {
  /**
   * @defaultValue 'vertical'
   */
  orientation?: 'horizontal' | 'vertical'
  avatar?: AvatarProps
}
</script>
<script setup lang="ts">
defineProps<AlertProps>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Alert.vue");
    const orientationProp = meta.props.find((prop) => prop.name === "orientation");
    expect(orientationProp).toBeDefined();
    expect(orientationProp!.type).toBe('"vertical" | "horizontal" | undefined');
    expect(orientationProp!.schema).toEqual({
      kind: "enum",
      type: '"vertical" | "horizontal" | undefined',
      schema: ['"horizontal"', '"vertical"', "undefined"],
    });

    const avatarProp = meta.props.find((prop) => prop.name === "avatar");
    expect(avatarProp).toBeDefined();
    // The native payload resolves AvatarProps structurally (not as a named ref).
    // The schema is an enum with an object entry containing the structural members
    // and "undefined" for the optional marker.
    expect(typeof avatarProp!.schema).not.toBe("string");
    if (
      typeof avatarProp!.schema !== "string" &&
      !Array.isArray(avatarProp!.schema) &&
      avatarProp!.schema.kind === "enum"
    ) {
      const avatarObject = avatarProp!.schema.schema.find(
        (entry): entry is Extract<typeof entry, { kind: "object" }> =>
          typeof entry !== "string" &&
          !Array.isArray(entry) &&
          entry.kind === "object",
      );
      expect(avatarObject).toBeDefined();
      // The chip member is present as a structurally-expanded property.
      // ChipProps itself is an opaque ref with empty schema (no JS resolver).
      const chipEntry = (avatarObject!.schema as Record<string, any>).chip;
      expect(chipEntry).toBeDefined();
      expect(chipEntry.schema.kind).toBe("enum");
      const chipObjectRef = chipEntry.schema.schema.find(
        (entry: any) =>
          typeof entry !== "string" &&
          !Array.isArray(entry) &&
          entry.kind === "object" &&
          entry.type === "ChipProps",
      );
      expect(chipObjectRef).toBeDefined();
      expect(chipObjectRef.schema).toEqual({});
    }

    checker.close();
  });
});

describe("MetaCheckerOptions logging.audit plumbing", () => {
  it("accepts logging.audit option without error", async () => {
    // This test verifies the TS types accept the logging option and it
    // plumbs through to native config creation without throwing.
    // The actual native audit behavior is tested in Rust.
    const projectRoot = resolve(process.env.TEMP ?? "/tmp", `audit-plumb-test-${Date.now()}`);
    mkdirSync(projectRoot, { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({ compilerOptions: { strict: true } }),
    );
    writeFileSync(
      resolve(projectRoot, "Test.vue"),
      "<script setup lang='ts'>\ndefineProps<{ x: string }>()\n</script>\n<template><div/></template>",
    );

    // With audit: false (default behavior)
    const checkerOff = await createCheckerByJson(projectRoot, {
      include: [runtimeNormalizePath(resolve(projectRoot, "Test.vue"))],
      runtimeMode: "dedicated",
      logging: { audit: false },
    });
    expect(checkerOff).toBeDefined();
    checkerOff.close();

    // With audit: true
    const checkerOn = await createCheckerByJson(projectRoot, {
      include: [runtimeNormalizePath(resolve(projectRoot, "Test.vue"))],
      runtimeMode: "dedicated",
      logging: { audit: true },
    });
    expect(checkerOn).toBeDefined();
    checkerOn.close();
  });

  it("defaults logging to undefined when not specified", async () => {
    const projectRoot = resolve(process.env.TEMP ?? "/tmp", `audit-default-test-${Date.now()}`);
    mkdirSync(projectRoot, { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({ compilerOptions: { strict: true } }),
    );
    writeFileSync(
      resolve(projectRoot, "Test.vue"),
      "<script setup lang='ts'>\ndefineProps<{ x: string }>()\n</script>\n<template><div/></template>",
    );

    // No logging option at all
    const checker = await createCheckerByJson(projectRoot, {
      include: [runtimeNormalizePath(resolve(projectRoot, "Test.vue"))],
      runtimeMode: "dedicated",
    });
    expect(checker).toBeDefined();
    checker.close();
  });
});
