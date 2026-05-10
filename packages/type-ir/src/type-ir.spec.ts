import { describe, expect, it } from "vitest";

import {
  indexedAccess,
  literal,
  primitive,
  ref,
  type IndexedAccessType,
  type TypeDescriptor,
} from "./index.js";

describe("IndexedAccessType", () => {
  it("constructs an indexed-access descriptor with object/index sub-shapes", () => {
    const descriptor: IndexedAccessType = indexedAccess(ref("Foo"), literal("slots"));

    expect(descriptor).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Foo" },
      indexType: { kind: "literal", value: "slots" },
    });
  });

  it("is included in the TypeDescriptor discriminated union", () => {
    // The descriminator is structural: a value typed as `TypeDescriptor`
    // must accept the `indexedAccess` kind without an `as` cast. If the
    // variant were missing from the union, this assignment would fail
    // at compile time (W0.6's primary contract — pre-cutover this file
    // would not type-check).
    const td: TypeDescriptor = indexedAccess(ref("NuxtLinkProps"), literal("to"));

    expect(td.kind).toBe("indexedAccess");
    if (td.kind === "indexedAccess") {
      expect(td.objectType.kind).toBe("ref");
      expect(td.indexType.kind).toBe("literal");
    }
  });

  it("supports nested indexed access (T['a']['b'])", () => {
    const inner = indexedAccess(ref("Theme"), literal("color"));
    const outer = indexedAccess(inner, literal("primary"));

    expect(outer).toEqual({
      kind: "indexedAccess",
      objectType: {
        kind: "indexedAccess",
        objectType: { kind: "ref", name: "Theme" },
        indexType: { kind: "literal", value: "color" },
      },
      indexType: { kind: "literal", value: "primary" },
    });
  });

  it("supports non-literal index types (T[K] where K is a generic param)", () => {
    // The index type is not constrained to literals — the W7.2 rewrite
    // of `looksLikeIndexedAccessType` keys off `t.kind === "indexedAccess"`,
    // not on the literalness of the index. A generic-parameter index
    // must round-trip through the descriptor.
    const descriptor = indexedAccess(ref("T"), ref("K"));

    expect(descriptor.objectType).toEqual({ kind: "ref", name: "T" });
    expect(descriptor.indexType).toEqual({ kind: "ref", name: "K" });
  });

  it("does not collapse to primitive/unknown shapes", () => {
    const descriptor = indexedAccess(ref("Foo"), literal("bar"));

    // Negative assertions per CLAUDE.md "always include negative assertions":
    // verify the variant is structurally distinct from neighbouring kinds.
    expect(descriptor.kind).not.toBe("unknown");
    expect(descriptor.kind).not.toBe("ref");
    expect(descriptor.kind).not.toBe("primitive");
    expect(descriptor).not.toHaveProperty("rawType");
    expect(descriptor).not.toHaveProperty("name");
  });

  it("the audit-flagged W7.2 predicates discriminate against the new variant", () => {
    // W7.2 will replace the regex-based `looksLikeIndexedAccessType`
    // with a structural predicate. The post-W0.6 test below is exactly
    // the predicate body W7.2 will use; it MUST yield `true` for the
    // new variant and `false` for non-indexed-access shapes.
    const isIndexedAccess = (t: TypeDescriptor) => t.kind === "indexedAccess";

    expect(isIndexedAccess(indexedAccess(ref("Foo"), literal("slots")))).toBe(true);
    expect(isIndexedAccess(ref("Foo"))).toBe(false);
    expect(isIndexedAccess(primitive("string"))).toBe(false);

    // The W7.2 slots/ui helper predicates additionally key off the
    // index type being a string-literal with a fixed value.
    const isSlotsHelper = (t: TypeDescriptor) =>
      t.kind === "indexedAccess" && t.indexType.kind === "literal" && t.indexType.value === "slots";

    expect(isSlotsHelper(indexedAccess(ref("ButtonComponent"), literal("slots")))).toBe(true);
    expect(isSlotsHelper(indexedAccess(ref("ButtonComponent"), literal("ui")))).toBe(false);
    expect(isSlotsHelper(ref("ButtonComponent"))).toBe(false);
  });
});
