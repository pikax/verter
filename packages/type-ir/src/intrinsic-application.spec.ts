import { describe, expect, it } from "vitest";

import {
  intrinsicApplication,
  intrinsicDisplayName,
  primitive,
  ref,
  type IntrinsicApplicationType,
  type TypeDescriptor,
} from "./index.js";

describe("IntrinsicApplicationType", () => {
  it("constructs an applied compiler intrinsic carrying the wire op", () => {
    const descriptor: IntrinsicApplicationType = intrinsicApplication("awaited", [ref("T")]);

    expect(descriptor).toEqual({
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [{ kind: "ref", name: "T" }],
    });
  });

  it("is included in the TypeDescriptor discriminated union", () => {
    // The variant's primary contract: if it were missing from the union this
    // assignment would fail at compile time and this file would not build.
    const td: TypeDescriptor = intrinsicApplication("awaited", [primitive("string")]);

    expect(td.kind).toBe("intrinsicApplication");
  });

  it("defaults to no operands rather than requiring an empty array", () => {
    expect(intrinsicApplication("awaited")).toEqual({
      kind: "intrinsicApplication",
      op: "awaited",
      arguments: [],
    });
  });

  it("is NOT a ref, even though both spell `Awaited`", () => {
    const authored = ref("Awaited", [ref("T")]);
    const resolved = intrinsicApplication("awaited", [ref("T")]);

    // Same rendering, different objects. Collapsing them would let a userland
    // declaration named `Awaited` capture a compiler-native operation.
    expect(resolved).not.toEqual(authored);
    expect(resolved.kind).not.toBe(authored.kind);
  });

  it("keeps the wire token separate from the display spelling", () => {
    // Mirrors the Rust split between `wire_str()` and `display_name()`: the
    // wire form is stable and lowercase, the display form is what a reader
    // expects to see. A rendering change must never move the wire form.
    expect(intrinsicApplication("awaited").op).toBe("awaited");
    expect(intrinsicDisplayName("awaited")).toBe("Awaited");
  });

  it("renders an unrecognised op as its raw token instead of throwing", () => {
    // A newer producer's operation should degrade to an honest spelling — a
    // display helper is the wrong place to fail a build.
    expect(intrinsicDisplayName("uppercase")).toBe("uppercase");
  });
});
