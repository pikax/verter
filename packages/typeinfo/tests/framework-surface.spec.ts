/**
 * Decode specs for the framework-surface wire payload.
 *
 * Discriminating coverage for {@link decodeFrameworkSurfaceResponse}:
 * - the `framework_surface` arm decodes to a `FrameworkSurface` with a
 *   per-kind map carrying the resolved status + member names;
 * - the `error` arm decodes to a typed `FrameworkSurfaceError`;
 * - SUPPORTED-empty (a supported kind with zero members) is DISTINCT
 *   from UNSUPPORTED on the decoded surface;
 * - member names resolve through the graph string table (not raw ids).
 *
 * The encoded bytes the specs feed in are produced exactly as the
 * native binding produces them (`toBinary(TypeInfoGraphResponseSchema)`),
 * so a decode regression against the wire shape surfaces here.
 */

import { create, toBinary } from "@bufbuild/protobuf";
import {
  FrameworkSurfaceDeclarationKind,
  FrameworkSurfaceKind,
  FrameworkSurfaceKindSupport,
  FrameworkSurfaceOriginHopKind,
  FrameworkTag,
  GraphPrimitiveKind,
  TypeInfoGraphResponseSchema,
} from "@verter/proto";
import { describe, expect, it } from "vitest";

import {
  decodeFrameworkSurfaceResponse,
  type FrameworkSurface,
  type FrameworkSurfaceError,
} from "../src/framework-surface.js";

/**
 * Build the encoded `TypeInfoGraphResponse` bytes for a
 * `framework_surface` payload with the given graph string table and
 * per-kind entries — mirroring exactly what the native binding returns.
 */
function encodeFrameworkSurfaceResponse(
  strings: string[],
  surfaces: Array<{
    kind: FrameworkSurfaceKind;
    support: FrameworkSurfaceKindSupport;
    members: Array<{ nameId: number; required: boolean; readonly: boolean }>;
    diagnostics?: number[];
  }>,
): Uint8Array {
  const response = create(TypeInfoGraphResponseSchema, {
    kind: {
      case: "frameworkSurface",
      value: {
        schemaVersion: 3,
        framework: FrameworkTag.VUE,
        graph: { strings: { entries: strings } },
        surfaces: surfaces.map((s) => ({
          kind: s.kind,
          members: s.members.map((m) => ({
            nameId: m.nameId,
            typeNodeId: 0,
            required: m.required,
            readonly: m.readonly,
          })),
          status: {
            support: s.support,
            exactness: 0,
            diagnostics: (s.diagnostics ?? []).map((id) => ({
              severity: 0,
              messageNameId: id,
              spanCanonicalNameId: 0,
              spanStart: 0,
              spanEnd: 0,
              hasSpan: false,
            })),
          },
        })),
      },
    },
  });
  return toBinary(TypeInfoGraphResponseSchema, response);
}

describe("decodeFrameworkSurfaceResponse", () => {
  it("decodes the framework arm with per-kind status and member names", () => {
    const bytes = encodeFrameworkSurfaceResponse(
      ["", "count", "label"],
      [
        {
          kind: FrameworkSurfaceKind.PROPS,
          support: FrameworkSurfaceKindSupport.SUPPORTED,
          members: [
            { nameId: 1, required: true, readonly: false },
            { nameId: 2, required: false, readonly: true },
          ],
        },
      ],
    );

    const decoded = decodeFrameworkSurfaceResponse(bytes);
    expect("error" in decoded).toBe(false);
    const surface = decoded as FrameworkSurface;
    expect(surface.framework).toBe(FrameworkTag.VUE);

    const props = surface.kinds.get(FrameworkSurfaceKind.PROPS);
    expect(props).toBeDefined();
    expect(props!.support).toBe(FrameworkSurfaceKindSupport.SUPPORTED);
    expect(props!.members).toEqual([
      { name: "count", required: true, readonly: false },
      { name: "label", required: false, readonly: true },
    ]);
  });

  it("decodes a callable member's parameter/return structure from the payload graph", () => {
    // REGRESSION — the public operation DTO is type-bearing: a callback
    // member's graph node (callable object + signature) decodes to a
    // function descriptor. An opaque node cannot claim that complete
    // signature; a missing type_node_id (0) stays absent.
    const strings = ["", "onMove", "x", "y", "item", "default", "structurally unencodable"];
    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "frameworkSurface",
        value: {
          schemaVersion: 3,
          framework: FrameworkTag.SVELTE,
          graph: {
            strings: { entries: strings },
            nodes: [
              {},
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.NUMBER } } },
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.VOID } } },
              {
                kind: {
                  case: "object",
                  value: { callSignatureRefs: [0], constructSignatureRefs: [], members: [] },
                },
              },
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
              {
                kind: {
                  case: "object",
                  value: {
                    members: [
                      {
                        valueNodeId: 4,
                        optional: false,
                        readonly: false,
                        propertyKey: { key: { case: "stringId", value: 4 } },
                      },
                    ],
                    callSignatureRefs: [],
                    constructSignatureRefs: [],
                  },
                },
              },
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.UNKNOWN } } },
              {
                kind: {
                  case: "object",
                  value: { callSignatureRefs: [1], constructSignatureRefs: [], members: [] },
                },
              },
              {
                kind: {
                  case: "opaque",
                  value: {
                    error: { kind: { case: "other", value: { messageNameId: 6 } } },
                  },
                },
              },
            ],
            signatures: [
              {
                parameters: [
                  { nameId: 2, typeNodeId: 1, optional: false, rest: false },
                  { nameId: 3, typeNodeId: 1, optional: false, rest: false },
                ],
                returnTypeNodeId: 2,
              },
              {
                parameters: [{ nameId: 0, typeNodeId: 5, optional: false, rest: false }],
                returnTypeNodeId: 6,
              },
            ],
          },
          surfaces: [
            {
              kind: FrameworkSurfaceKind.PROPS,
              members: [
                { nameId: 1, typeNodeId: 3, required: true, readonly: false },
                { nameId: 0, typeNodeId: 8, required: true, readonly: false },
              ],
              status: { support: FrameworkSurfaceKindSupport.SUPPORTED, exactness: 0 },
            },
            {
              kind: FrameworkSurfaceKind.SLOTS,
              members: [{ nameId: 5, typeNodeId: 7, required: true, readonly: false }],
              status: { support: FrameworkSurfaceKindSupport.SUPPORTED, exactness: 0 },
            },
          ],
        },
      },
    });
    const bytes = toBinary(TypeInfoGraphResponseSchema, response);
    const surface = decodeFrameworkSurfaceResponse(bytes) as FrameworkSurface;

    const onMove = surface.kinds.get(FrameworkSurfaceKind.PROPS)!.members[0]!;
    expect(onMove.name).toBe("onMove");
    expect(onMove.type).toEqual({
      kind: "function",
      parameters: [
        { name: "x", type: { kind: "primitive", name: "number" }, optional: false },
        { name: "y", type: { kind: "primitive", name: "number" }, optional: false },
      ],
      returnType: { kind: "primitive", name: "void" },
    });

    const opaque = surface.kinds.get(FrameworkSurfaceKind.PROPS)!.members[1]!;
    expect(opaque.type?.kind).toBe("unknown");
    expect(opaque.type).not.toMatchObject({ kind: "function" });

    const slot = surface.kinds.get(FrameworkSurfaceKind.SLOTS)!.members[0]!;
    expect(slot.name).toBe("default");
    expect(slot.type?.kind).toBe("function");
    if (slot.type?.kind !== "function") throw new Error("function slot expected");
    expect(slot.type.parameters[0]!.type).toEqual({
      kind: "object",
      properties: [{ name: "item", type: { kind: "primitive", name: "string" }, optional: false }],
    });
  });

  it("decodes an absent signature return as unknown, not fabricated void", () => {
    // REGRESSION — returnTypeNodeId 0 is the producer's honest-absence
    // sentinel (NamedTypeMemberOutput::Function.return_type None), not a
    // resolved primitive void. Fabricating void is a wrong-complete claim.
    const strings = ["", "onTick", "n"];
    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "frameworkSurface",
        value: {
          schemaVersion: 3,
          framework: FrameworkTag.SVELTE,
          graph: {
            strings: { entries: strings },
            nodes: [
              {},
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.NUMBER } } },
              {
                kind: {
                  case: "object",
                  value: { callSignatureRefs: [0], constructSignatureRefs: [], members: [] },
                },
              },
            ],
            signatures: [
              {
                parameters: [{ nameId: 2, typeNodeId: 1, optional: false, rest: false }],
                returnTypeNodeId: 0,
              },
            ],
          },
          surfaces: [
            {
              kind: FrameworkSurfaceKind.PROPS,
              members: [{ nameId: 1, typeNodeId: 2, required: true, readonly: false }],
              status: { support: FrameworkSurfaceKindSupport.SUPPORTED, exactness: 0 },
            },
          ],
        },
      },
    });
    const surface = decodeFrameworkSurfaceResponse(
      toBinary(TypeInfoGraphResponseSchema, response),
    ) as FrameworkSurface;
    const onTick = surface.kinds.get(FrameworkSurfaceKind.PROPS)!.members[0]!;
    expect(onTick.name).toBe("onTick");
    expect(onTick.type).toEqual({
      kind: "function",
      parameters: [{ name: "n", type: { kind: "primitive", name: "number" }, optional: false }],
      returnType: { kind: "unknown", rawType: "absent" },
    });
    expect(onTick.type).not.toMatchObject({
      returnType: { kind: "primitive", name: "void" },
    });
  });

  it("decodes a rest parameter as a ...-prefixed name", () => {
    // REGRESSION — GraphSignatureParameter.rest is producer-populated; dropping
    // it made a variadic leaf-typed callback decode as fixed arity. type-ir
    // FunctionParameter has no rest field, so rest-ness is the name prefix.
    const strings = ["", "logAll", "args"];
    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "frameworkSurface",
        value: {
          schemaVersion: 3,
          framework: FrameworkTag.SVELTE,
          graph: {
            strings: { entries: strings },
            nodes: [
              {},
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
              { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.VOID } } },
              {
                kind: {
                  case: "object",
                  value: { callSignatureRefs: [0], constructSignatureRefs: [], members: [] },
                },
              },
            ],
            signatures: [
              {
                parameters: [{ nameId: 2, typeNodeId: 1, optional: false, rest: true }],
                returnTypeNodeId: 2,
              },
            ],
          },
          surfaces: [
            {
              kind: FrameworkSurfaceKind.PROPS,
              members: [{ nameId: 1, typeNodeId: 3, required: true, readonly: false }],
              status: { support: FrameworkSurfaceKindSupport.SUPPORTED, exactness: 0 },
            },
          ],
        },
      },
    });
    const surface = decodeFrameworkSurfaceResponse(
      toBinary(TypeInfoGraphResponseSchema, response),
    ) as FrameworkSurface;
    const logAll = surface.kinds.get(FrameworkSurfaceKind.PROPS)!.members[0]!;
    expect(logAll.type).toEqual({
      kind: "function",
      parameters: [
        { name: "...args", type: { kind: "primitive", name: "string" }, optional: false },
      ],
      returnType: { kind: "primitive", name: "void" },
    });
  });

  it("keeps SUPPORTED-empty distinct from UNSUPPORTED", () => {
    // A pure DECODE test: the decoder must surface the per-kind status verbatim
    // and keep SUPPORTED-empty (a real-but-empty surface) distinct from
    // UNSUPPORTED (a kind outside the adapter's supported set — e.g. a Deferred
    // adapter answering every kind structurally unsupported). Both carry zero
    // members; only the status discriminates them.
    const bytes = encodeFrameworkSurfaceResponse(
      ["surface kind not supported by this adapter"],
      [
        {
          kind: FrameworkSurfaceKind.SLOTS,
          support: FrameworkSurfaceKindSupport.SUPPORTED,
          members: [],
        },
        {
          kind: FrameworkSurfaceKind.EXPOSE,
          support: FrameworkSurfaceKindSupport.UNSUPPORTED,
          members: [],
          diagnostics: [0],
        },
      ],
    );

    const surface = decodeFrameworkSurfaceResponse(bytes) as FrameworkSurface;

    const slots = surface.kinds.get(FrameworkSurfaceKind.SLOTS)!;
    expect(slots.support).toBe(FrameworkSurfaceKindSupport.SUPPORTED);
    expect(slots.members).toEqual([]);
    // SUPPORTED-empty: zero members but a SUPPORTED status — a real
    // surface that happens to be empty, NOT unsupport.
    expect(slots.isSupported).toBe(true);
    expect(slots.isUnsupported).toBe(false);

    const unsupported = surface.kinds.get(FrameworkSurfaceKind.EXPOSE)!;
    expect(unsupported.support).toBe(FrameworkSurfaceKindSupport.UNSUPPORTED);
    expect(unsupported.members).toEqual([]);
    // UNSUPPORTED: also zero members, but the status discriminates it
    // from the supported-empty SLOTS surface above.
    expect(unsupported.isSupported).toBe(false);
    expect(unsupported.isUnsupported).toBe(true);
    expect(unsupported.diagnostics).toEqual(["surface kind not supported by this adapter"]);
  });

  it("decodes OPTIONS and EXPOSE as SUPPORTED with their members", () => {
    // `defineOptions<T>()` / `defineExpose<T>()` resolve SUPPORTED with the
    // type-argument members — the decoder surfaces them like any other
    // object-member surface (NOT a special unsupported-because-present case).
    const bytes = encodeFrameworkSurfaceResponse(
      ["", "name", "focus"],
      [
        {
          kind: FrameworkSurfaceKind.OPTIONS,
          support: FrameworkSurfaceKindSupport.SUPPORTED,
          members: [{ nameId: 1, required: true, readonly: false }],
        },
        {
          kind: FrameworkSurfaceKind.EXPOSE,
          support: FrameworkSurfaceKindSupport.SUPPORTED,
          members: [{ nameId: 2, required: true, readonly: false }],
        },
      ],
    );

    const surface = decodeFrameworkSurfaceResponse(bytes) as FrameworkSurface;

    const options = surface.kinds.get(FrameworkSurfaceKind.OPTIONS)!;
    expect(options.support).toBe(FrameworkSurfaceKindSupport.SUPPORTED);
    expect(options.isSupported).toBe(true);
    expect(options.members).toEqual([{ name: "name", required: true, readonly: false }]);

    const expose = surface.kinds.get(FrameworkSurfaceKind.EXPOSE)!;
    expect(expose.support).toBe(FrameworkSurfaceKindSupport.SUPPORTED);
    expect(expose.isSupported).toBe(true);
    expect(expose.members).toEqual([{ name: "focus", required: true, readonly: false }]);
  });

  it("exposes a member's runtime default and member-declaration origin (schema 4)", () => {
    // THE P0 PUBLIC-CONSUMER PROOF (DISCRIMINATING): a docs / semantic-DB
    // consumer decoding the framework-surface wire reads each prop's runtime
    // DEFAULT value source text AND its member-declaration ORIGIN through this
    // public surface. Built directly from the wire `FrameworkSurfaceMember`
    // `default_value_id` + `origin` (schema 4) — RED before the wire carried
    // them (the fields did not exist on the member shape).
    //
    // The string table's entry 0 is a DISTINCTIVE non-empty sentinel — the
    // real encoder NEVER seeds entry 0 with `""`. A LOCAL hop carries none of
    // the import/reexport/alias string-id fields; the presence-aware decoder
    // must decode those absent fields as `undefined`, NOT as string-table
    // entry 0 (the zero-based-table-vs-0-sentinel collision this guards).
    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "frameworkSurface",
        value: {
          schemaVersion: 4,
          framework: FrameworkTag.SVELTE,
          graph: {
            schemaVersion: 4,
            strings: { entries: ["__SENTINEL_ZERO__", "size", "label", "'md'", "/App.svelte"] },
          },
          surfaces: [
            {
              kind: FrameworkSurfaceKind.PROPS,
              members: [
                {
                  // `size = 'md'` — optional, carries a default + a LOCAL origin.
                  nameId: 1,
                  typeNodeId: 0,
                  required: false,
                  readonly: false,
                  defaultValueId: 3,
                  origin: {
                    declaration: {
                      requestedNameId: 1,
                      resolvedNameId: 1,
                      canonicalSourceId: 4,
                      spanStart: 0,
                      spanEnd: 0,
                      kind: FrameworkSurfaceDeclarationKind.UNKNOWN,
                    },
                    // A LOCAL hop sets NO string-id field — every hop string
                    // id is absent (the `optional` has-bit is unset).
                    chain: [{ kind: FrameworkSurfaceOriginHopKind.LOCAL }],
                  },
                },
                // `label` — required, NO default (the `default_value_id`
                // `optional` field is absent), NO origin.
                { nameId: 2, typeNodeId: 0, required: true, readonly: false },
              ],
              status: {
                support: FrameworkSurfaceKindSupport.SUPPORTED,
                exactness: 0,
                diagnostics: [],
              },
            },
          ],
        },
      },
    });
    const bytes = toBinary(TypeInfoGraphResponseSchema, response);

    const surface = decodeFrameworkSurfaceResponse(bytes) as FrameworkSurface;
    const props = surface.kinds.get(FrameworkSurfaceKind.PROPS)!;
    const size = props.members.find((m) => m.name === "size")!;
    expect(size.default).toBe("'md'");
    expect(size.origin).toBeDefined();
    expect(size.origin!.chain).toHaveLength(1);
    const localHop = size.origin!.chain[0];
    expect(localHop.kind).toBe(FrameworkSurfaceOriginHopKind.LOCAL);
    // DISCRIMINATING (the P0): a LOCAL hop's import/reexport/alias string
    // fields are genuinely ABSENT — never resolved through the string table
    // to entry 0 (`"__SENTINEL_ZERO__"`).
    expect(localHop.from).toBeUndefined();
    expect(localHop.specifier).toBeUndefined();
    expect(localHop.importedName).toBeUndefined();
    expect(localHop.to).toBeUndefined();
    expect(localHop.exportedName).toBeUndefined();
    expect(localHop.originalName).toBeUndefined();
    expect(localHop.aliasName).toBeUndefined();
    expect(size.origin!.declaration).toBeDefined();
    expect(size.origin!.declaration!.canonicalSource).toBe("/App.svelte");
    expect(size.origin!.declaration!.resolvedName).toBe("size");

    // DISCRIMINATING: a member without a default decodes `default` to
    // `undefined` (presence-aware), and no origin → `undefined`.
    const label = props.members.find((m) => m.name === "label")!;
    expect(label.default).toBeUndefined();
    expect(label.origin).toBeUndefined();
  });

  it("decodes an IMPORT hop without a specifier as an absent specifier (schema 4)", () => {
    // DISCRIMINATING (the P0, import variant): an IMPORT hop carries `from` +
    // `importedName` but may have NO recorded `specifier`. The presence-aware
    // decoder must surface `from`/`importedName` as their interned strings and
    // `specifier` as ABSENT — never string-table entry 0 (the distinctive
    // sentinel below), and never the bogus specifier the old id-0 decode
    // produced.
    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "frameworkSurface",
        value: {
          schemaVersion: 4,
          framework: FrameworkTag.SVELTE,
          graph: {
            schemaVersion: 4,
            strings: { entries: ["__SENTINEL_ZERO__", "size", "/lib/props.ts", "Size"] },
          },
          surfaces: [
            {
              kind: FrameworkSurfaceKind.PROPS,
              members: [
                {
                  nameId: 1,
                  typeNodeId: 0,
                  required: false,
                  readonly: false,
                  origin: {
                    declaration: {
                      requestedNameId: 1,
                      resolvedNameId: 3,
                      canonicalSourceId: 2,
                      spanStart: 0,
                      spanEnd: 0,
                      kind: FrameworkSurfaceDeclarationKind.TYPE_ALIAS,
                    },
                    // IMPORT hop: `from` + `importedName` present, NO specifier.
                    chain: [
                      {
                        kind: FrameworkSurfaceOriginHopKind.IMPORT,
                        fromId: 2,
                        importedNameId: 3,
                      },
                    ],
                  },
                },
              ],
              status: {
                support: FrameworkSurfaceKindSupport.SUPPORTED,
                exactness: 0,
                diagnostics: [],
              },
            },
          ],
        },
      },
    });
    const bytes = toBinary(TypeInfoGraphResponseSchema, response);

    const surface = decodeFrameworkSurfaceResponse(bytes) as FrameworkSurface;
    const props = surface.kinds.get(FrameworkSurfaceKind.PROPS)!;
    const size = props.members.find((m) => m.name === "size")!;
    const importHop = size.origin!.chain[0];
    expect(importHop.kind).toBe(FrameworkSurfaceOriginHopKind.IMPORT);
    expect(importHop.from).toBe("/lib/props.ts");
    expect(importHop.importedName).toBe("Size");
    // DISCRIMINATING: an unrecorded specifier is ABSENT, never the bogus
    // string-table entry 0.
    expect(importHop.specifier).toBeUndefined();
  });

  it("decodes the error arm to a typed FrameworkSurfaceError", () => {
    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "error",
        value: {
          kind: {
            case: "malformedPayload",
            value: { detail: "unknown adapter id" },
          },
        },
      },
    });
    const bytes = toBinary(TypeInfoGraphResponseSchema, response);

    const decoded = decodeFrameworkSurfaceResponse(bytes);
    expect("error" in decoded).toBe(true);
    const err = decoded as FrameworkSurfaceError;
    // The error arm decodes the TYPED oneof variant, not a stringified
    // display: `case` is the wire variant, `value` its typed payload.
    expect(err.error.case).toBe("malformedPayload");
    if (err.error.case === "malformedPayload") {
      expect(err.error.value.detail).toBe("unknown adapter id");
    }
  });
});
