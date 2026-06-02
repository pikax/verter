import { describe, expect, it } from "vitest";

import type { NativeTypeExpr } from "./type-expr-bridge.js";
import { typeExprToDescriptor } from "./type-expr-bridge.js";

/**
 * Rust's `to_json_value` serialises a bare constructor type
 * (`TypeExpr::ConstructorType`) with `kind: "constructorType"` — the same
 * payload (parameters / returnType / typeParameters) as `kind: "function"`.
 * The component-meta native→descriptor bridge must map it function-like (a
 * `FunctionType` descriptor), exactly like `@verter/typeinfo`'s
 * `nativeToDescriptor`. The constructor-vs-function distinction is consumed
 * in Rust before the wire, so function-like is the contract-correct shape.
 *
 * Discriminating: before `constructorType` was modelled + cased, the node
 * fell to the `default:` arm of `typeExprToDescriptor` and degraded to
 * `unknown("unrecognized")`.
 */
describe("typeExprToDescriptor — constructorType", () => {
  it("maps a constructorType native node to a function descriptor", () => {
    // `new (x: string) => Foo`
    const ctor: NativeTypeExpr = {
      kind: "constructorType",
      parameters: [{ name: "x", ty: { kind: "primitive", name: "string" }, optional: false }],
      returnType: { kind: "ref", name: "Foo", typeArguments: [] },
    };

    const descriptor = typeExprToDescriptor(ctor);

    expect(descriptor.kind).toBe("function");
    if (descriptor.kind !== "function") {
      throw new Error("unreachable — kind narrowed above");
    }
    expect(descriptor.parameters).toHaveLength(1);
    expect(descriptor.parameters[0]!.name).toBe("x");
    expect(descriptor.returnType.kind).toBe("ref");
  });

  it("does NOT degrade a constructorType node to unknown('unrecognized')", () => {
    const ctor: NativeTypeExpr = {
      kind: "constructorType",
      parameters: [],
      returnType: { kind: "primitive", name: "void" },
    };

    const descriptor = typeExprToDescriptor(ctor);

    // The pre-fix `default:` arm returned `unknown("unrecognized")`.
    expect(descriptor.kind).not.toBe("unknown");
    expect(descriptor.kind).toBe("function");
  });

  it("maps a plain function native node identically (parity control)", () => {
    const fn: NativeTypeExpr = {
      kind: "function",
      parameters: [{ name: "x", ty: { kind: "primitive", name: "string" }, optional: false }],
      returnType: { kind: "ref", name: "Foo", typeArguments: [] },
    };

    const descriptor = typeExprToDescriptor(fn);
    expect(descriptor.kind).toBe("function");
  });
});
