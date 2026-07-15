/**
 * TS native-bridge regression — `constructorType` lowering.
 *
 * Rust serialises a bare TS constructor type (`new (...) => R`,
 * `TypeExpr::ConstructorType`) to a wire node with `kind: "constructorType"`
 * and the SAME payload shape as `function` (`parameters` / `returnType` /
 * `typeParameters` via `function_to_json`). Before this case existed the TS
 * union + `nativeToDescriptor` switch had no `constructorType` arm, so a raw
 * native node went through unhandled (`undefined` descriptor at runtime).
 *
 * The bridge treats a `constructorType` node function-like: it maps to the
 * `FunctionType` descriptor (`kind: "function"`), consistent with the bound
 * wire-graph decision and the Rust-side lower→raise treatment. The
 * constructor-vs-function distinction is consumed in Rust (Vue runtime-ctor
 * reducer + wire-graph builder) before this bridge.
 *
 * Discrimination: against the pre-fix bridge (no `constructorType` case in the
 * `nativeToDescriptor` switch), `nativeToDescriptor` returns `undefined` for
 * this node and the `kind === "function"` assertion throws — the test FAILS.
 */

import { describe, expect, it } from "vitest";

// Import the pure mapper directly (NOT the `../src/index.js` barrel, which
// transitively pulls in `@verter/native` and needs the built `.node` binding).
import { nativeToDescriptor } from "../src/native-to-descriptor.js";
import type { NativeTypeExpr } from "../src/native-type-expr.js";

describe("nativeToDescriptor — constructorType", () => {
  it("maps a constructorType native node to a function-like descriptor", () => {
    // `new (x: string) => Foo` — a bare constructor type carrying one named
    // parameter and a `ref` return so both survive into the descriptor.
    const node: NativeTypeExpr = {
      kind: "constructorType",
      parameters: [
        {
          name: "x",
          ty: { kind: "primitive", name: "string" },
          optional: false,
          rest: false,
        },
      ],
      returnType: { kind: "ref", name: "Foo", typeArguments: [] },
      typeParameters: [],
    };

    const descriptor = nativeToDescriptor(node);

    // Function-like: NOT undefined, NOT `unknown(raw)` — a real FunctionType.
    expect(descriptor).toBeDefined();
    expect(descriptor.kind).toBe("function");

    if (descriptor.kind !== "function") {
      throw new Error(`expected a function descriptor, got ${descriptor.kind}`);
    }

    // The parameter survives lowering with its name + lowered primitive type.
    expect(descriptor.parameters).toHaveLength(1);
    expect(descriptor.parameters[0]?.name).toBe("x");
    expect(descriptor.parameters[0]?.type).toEqual({
      kind: "primitive",
      name: "string",
    });

    // The return type lowers to the referenced descriptor (not `void`).
    expect(descriptor.returnType).toEqual({
      kind: "ref",
      name: "Foo",
      typeArguments: [],
    });
  });

  it("lowers a constructorType identically to the same-payload function node", () => {
    // The constructor-vs-function distinction is erased at this bridge: a
    // `constructorType` and a `function` with the SAME payload must produce
    // structurally-equal descriptors.
    const payload = {
      parameters: [
        {
          name: "a",
          ty: { kind: "primitive" as const, name: "number" as const },
          optional: false,
          rest: false,
        },
      ],
      returnType: { kind: "primitive" as const, name: "boolean" as const },
      typeParameters: [],
    };

    const ctorNode: NativeTypeExpr = { kind: "constructorType", ...payload };
    const fnNode: NativeTypeExpr = { kind: "function", ...payload };

    expect(nativeToDescriptor(ctorNode)).toEqual(nativeToDescriptor(fnNode));
  });
});
