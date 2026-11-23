import { describe, it, expect } from "vitest";
import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type { NativeTypeExpr } from "./type-expr-bridge.js";
import { nativeComponentMetaToComponentMeta } from "./native-component-meta.js";
import {
  createGraphTypeExprRef,
  DecodedTypeGraph,
  INTRINSIC_OP_AWAITED,
  INTRINSIC_OP_NO_INFER,
  INTRINSIC_OP_UNSPECIFIED,
  NODE_INTRINSIC_APPLICATION,
  NODE_REF,
} from "./type-graph-core.js";
import { decodeComponentMetaPayload } from "./type-graph.js";
import { encodeTestComponentMetaPayload } from "./type-graph.test-utils.js";

// =============================================================================
// A compiler intrinsic must survive the native -> descriptor boundary intact.
//
// This is the point where the distinction is easiest to lose: before this
// variant existed, an unrecognised `kind` fell through to
// `unknown("unrecognized")`, which would have destroyed the intrinsic identity
// *before* any adapter or checker could honour it. These tests pin that it
// does not, and that it is never resolved as if it named a declaration.
// =============================================================================

const operand: NativeTypeExpr = { kind: "ref", name: "T", typeArguments: [] };

const resolvedAwaited: NativeTypeExpr = {
  kind: "intrinsicApplication",
  op: "awaited",
  arguments: [operand],
};

const authoredAwaited: NativeTypeExpr = {
  kind: "ref",
  name: "Awaited",
  typeArguments: [operand],
};

describe("typeExprToDescriptor: compiler intrinsics", () => {
  it("lowers to an intrinsicApplication descriptor, not to unknown", () => {
    const descriptor = typeExprToDescriptor(resolvedAwaited);

    expect(descriptor).toEqual({
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [{ kind: "ref", name: "T" }],
    });
  });

  it("never degrades to the `unrecognized` default", () => {
    // The anti-fallthrough pin. If someone removes the explicit arm, this
    // fails loudly here instead of silently producing an unknown shape that
    // every downstream consumer then renders as `unknown`.
    const descriptor = typeExprToDescriptor(resolvedAwaited);

    expect(descriptor.kind).not.toBe("unknown");
  });

  it("keeps the intrinsic distinct from the reference that renders the same", () => {
    expect(typeExprToDescriptor(resolvedAwaited)).not.toEqual(
      typeExprToDescriptor(authoredAwaited),
    );
  });

  it("lowers the operands recursively", () => {
    const nested: NativeTypeExpr = {
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [
        { kind: "array", element: { kind: "primitive", name: "number" }, readonly: false },
      ],
    };

    expect(typeExprToDescriptor(nested)).toEqual({
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [{ kind: "array", element: { kind: "primitive", name: "number" } }],
    });
  });

  it("carries an operand-free application through with no arguments", () => {
    expect(
      typeExprToDescriptor({ kind: "intrinsicApplication", op: "awaited", arguments: [] }),
    ).toEqual({ kind: "intrinsicApplication", op: "awaited", arguments: [] });
  });
});

describe("typeExprToDescriptor: graph-backed compiler intrinsics", () => {
  function intrinsicGraph(op: number): DecodedTypeGraph {
    return new DecodedTypeGraph(
      ["T"],
      [
        { kind: NODE_REF, nameId: 1, typeArgumentNodeIds: [] },
        { kind: NODE_INTRINSIC_APPLICATION, op, argumentNodeIds: [1] },
      ],
    );
  }

  it.each([
    [INTRINSIC_OP_AWAITED, "awaited"],
    [INTRINSIC_OP_NO_INFER, "noInfer"],
  ])("transcribes wire op %i to the %s token with its operands", (op, token) => {
    expect(typeExprToDescriptor(createGraphTypeExprRef(intrinsicGraph(op), 2))).toEqual({
      kind: "intrinsicApplication",
      op: token,
      arguments: [{ kind: "ref", name: "T" }],
    });
  });

  it("matches the plain native transcription of the same application", () => {
    expect(
      typeExprToDescriptor(createGraphTypeExprRef(intrinsicGraph(INTRINSIC_OP_AWAITED), 2)),
    ).toEqual(typeExprToDescriptor(resolvedAwaited));
  });

  it("keeps an op without a wire token as its printed text, never as Awaited", () => {
    expect(
      typeExprToDescriptor(createGraphTypeExprRef(intrinsicGraph(INTRINSIC_OP_UNSPECIFIED), 2)),
    ).toEqual({ kind: "unknown", rawType: "unknown intrinsic<T>" });
  });

  it("transcribes an intrinsic decoded from a component-meta payload", () => {
    const native = decodeComponentMetaPayload(
      encodeTestComponentMetaPayload({
        filePath: "/project/src/Loader.vue",
        props: [
          {
            name: "result",
            type: {
              kind: "intrinsicApplication",
              op: INTRINSIC_OP_AWAITED,
              arguments: [{ kind: "ref", name: "Pending" }],
            },
            required: true,
          },
        ],
      }),
    );

    expect(nativeComponentMetaToComponentMeta(native).props?.[0]?.type).toEqual({
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [{ kind: "ref", name: "Pending" }],
    });
  });
});
