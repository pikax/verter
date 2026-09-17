import { describe, it, expect } from "vitest";
import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type { NativeTypeExpr } from "./type-expr-bridge.js";

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

  it("does NOT resolve through the native registry, even when `Awaited` is registered", () => {
    // The contract that matters: an intrinsic names no declaration, so a
    // userland type registered under the same spelling must not capture it.
    const registry = new Map<string, NativeTypeExpr>([
      ["Awaited", { kind: "primitive", name: "string" }],
    ]);

    const resolved = typeExprToDescriptor(resolvedAwaited, registry);
    expect(resolved).toEqual({
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [{ kind: "ref", name: "T" }],
    });

    // Sanity check that the registry is live at all — an authored reference
    // spelled the same way DOES go through it. Without this, the assertion
    // above could pass for the wrong reason (an inert registry).
    const authored = typeExprToDescriptor(authoredAwaited, registry);
    expect(authored).not.toEqual(resolved);
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
