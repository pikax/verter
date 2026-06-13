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
  FrameworkSurfaceKind,
  FrameworkSurfaceKindSupport,
  FrameworkTag,
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
