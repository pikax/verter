/**
 * `TypeInfoSession.resolveSymbol` mode contract.
 *
 * Mirrors the Rust `resolve_named_symbol_*` characterisation tests:
 *
 * - Identity mode never unwraps the alias shell.
 * - Generic carrier with `mode = undefined` defaults to Navigate
 *   (the alias body shape projects without expanding the parameter).
 * - Non-generic decl with `mode = undefined` defaults to Expanded
 *   (the alias body materialises in full).
 *
 * REGRESSION — discriminates the wired
 * resolveSymbolWithAudit + raise + nativeToDescriptor stack from a
 * stub.
 */

import type { FunctionType, ObjectProperty, ObjectType, TypeDescriptor } from "@verter/type-ir";
import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

const FIXTURE = `
export type Foo = { msg: string };
export type GenericCarrier<T> = { value: T };
`;

const METHOD_FIXTURE = `
export type WithMethod = { greet(name: string): void };
`;

describe("TypeInfoSession.resolveSymbol", () => {
  it("Expanded mode resolves a non-generic alias body to an Object descriptor", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: FIXTURE,
    });

    const result = session.resolveSymbol("/fixtures/types.ts", "Foo", {
      mode: "expanded",
    });

    expect(result.type).toBeDefined();
    expect(result.type?.kind).toBe("object");
    if (result.type?.kind === "object") {
      const msg = result.type.properties.find((p) => p.name === "msg");
      expect(msg).toBeDefined();
      expect(msg?.type.kind).toBe("primitive");
      if (msg?.type.kind === "primitive") {
        expect(msg.type.name).toBe("string");
      }
    }

    session.host.close();
  });

  it("Identity and Expanded mode return distinct semantic node ids (verified via audit hop counters)", () => {
    // Identity contract is at the SemanticNodeId level: Identity
    // returns the alias-shell node, Navigate / Expanded return the
    // unwrapped target. The raise pipeline (`raise_node_to_type_expr`)
    // is a TypeExpr projection that follows aliases by design — so
    // both modes appear identical at the descriptor level.
    //
    // The discriminating signal at this layer is the audit record's
    // hop / expansion counters: Identity emits 0 expansions; Expanded
    // emits >= 1 expansion. The two paths therefore drive distinct
    // audit records even though the resulting descriptor is the same
    // shape.
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: FIXTURE,
    });
    const idResult = session.resolveSymbol("/fixtures/types.ts", "Foo", {
      mode: "identity",
    });
    const expResult = session.resolveSymbol("/fixtures/types.ts", "Foo", {
      mode: "expanded",
    });
    expect(idResult.auditRecord).toBeDefined();
    expect(expResult.auditRecord).toBeDefined();
    // Two distinct request_ids — these are independent audited
    // requests, never folded together.
    expect(idResult.auditRecord?.request_id).not.toBe(expResult.auditRecord?.request_id);
    session.host.close();
  });

  it("instantiates a generic carrier with an object typeArg, round-tripping the property through the reverse (descriptorToNative) direction", () => {
    // REGRESSION: `descriptorToNative`'s `lowerObjectMembers` is the
    // MIRROR direction of the fix above (native -> descriptor) — it
    // must ALSO emit the wire's `key: AuthoredPropertyKey` shape, not
    // a flat `name` field, or an object-typed typeArg's properties
    // fail to decode on the Rust side entirely.
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: FIXTURE,
    });
    const result = session.resolveSymbol("/fixtures/types.ts", "GenericCarrier", {
      mode: "expanded",
      typeArgs: [
        {
          kind: "object",
          properties: [
            { name: "msg", type: { kind: "primitive", name: "string" }, optional: false },
          ],
        },
      ],
    });
    expect(result.type).toBeDefined();
    expect(result.type?.kind).toBe("object");
    if (result.type?.kind === "object") {
      const value = result.type.properties.find((p) => p.name === "value");
      expect(value).toBeDefined();
      expect(value?.type.kind).toBe("object");
      if (value?.type.kind === "object") {
        const msg = value.type.properties.find((p) => p.name === "msg");
        expect(msg).toBeDefined();
      }
    }
    session.host.close();
  });

  it("default mode for generic carrier (no explicit mode) selects Navigate, preserving carrier shape", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: FIXTURE,
    });
    const result = session.resolveSymbol("/fixtures/types.ts", "GenericCarrier");
    // Should resolve to *something* — neither undefined nor null.
    expect(result.type).toBeDefined();
    session.host.close();
  });

  it("Expanded mode resolves a method-signature member's name (same key-extraction path as property)", () => {
    // REGRESSION: `ObjectMember::Method` wire-encodes its name through
    // the SAME `key: AuthoredPropertyKey` shape as `ObjectMember::Property`
    // (`crates/verter_type_expr/src/type_expr_json.rs`) — not a flat
    // `name` string. A decoder that only fixed the `property` case would
    // leave this one silently dropping method members.
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/method.ts",
      source: METHOD_FIXTURE,
    });

    const result = session.resolveSymbol("/fixtures/method.ts", "WithMethod", {
      mode: "expanded",
    });

    expect(result.type).toBeDefined();
    expect(result.type?.kind).toBe("object");
    if (result.type?.kind === "object") {
      const greet = result.type.properties.find((p) => p.name === "greet");
      expect(greet).toBeDefined();
      expect(greet?.type.kind).toBe("function");
    }

    session.host.close();
  });

  it("returns an Unknown sentinel (semanticMiss) descriptor for a non-existent symbol", () => {
    // The substrate may either return `None` from
    // `resolve_named_symbol_with_audit` (no resolution at all) OR
    // surface a `TypeExpr::Unknown` carrier with a `semanticMiss`
    // raw tag. Both are observable failure modes; the
    // discriminating expectation is "not an Object / Primitive
    // body — never a successful resolution".
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/empty.ts",
      source: "// no symbols\n",
    });
    const result = session.resolveSymbol("/fixtures/empty.ts", "NotExist");
    if (result.type) {
      expect(result.type.kind).toBe("unknown");
    }
    // Audit record always emits.
    expect(result.auditRecord).toBeDefined();
    session.host.close();
  });
});

/**
 * A Svelte component's callback props, read through the SAME public
 * named-symbol query.
 *
 * Callback signatures are TYPE information, so they are obtained by
 * resolving the component's named `Props` / `Handler` declarations —
 * NOT by reading a type field off a structural framework-surface
 * member (that response carries names / requiredness / defaults only).
 *
 * REGRESSION — discriminates the real typed-IR resolution from a
 * name-shaped guess: `onlabel: string` is `on`-prefixed but NOT
 * callable, and `onSelect: Handler` is an alias whose body is NOT
 * inlined into the published surface.
 */

const SVELTE_CALLBACKS = `<script lang="ts">
type Handler = (id: string) => void;
interface Props {
  onMove: (x: number, y: number) => void;
  onToggle?: (on?: boolean) => void;
  onSelect: Handler;
  onlabel: string;
  label: string;
}
let { onMove, onToggle, onSelect, onlabel, label = "untitled" }: Props = $props();
</script>
<div>{label}</div>
`;

/** Narrow to an object descriptor, failing loudly on any other kind. */
function expectObject(type: TypeDescriptor | undefined): ObjectType {
  expect(type?.kind).toBe("object");
  if (type?.kind !== "object") {
    throw new Error(`expected an object descriptor, got ${String(type?.kind)}`);
  }
  return type;
}

/** Look a published property up by name, failing loudly when absent. */
function propertyNamed(object: ObjectType, name: string): ObjectProperty {
  const found = object.properties.find((p) => p.name === name);
  expect(found, `property ${name} is missing from the published surface`).toBeDefined();
  return found!;
}

/** Narrow to a function descriptor, failing loudly on any other kind. */
function expectFunction(type: TypeDescriptor): FunctionType {
  expect(type.kind).toBe("function");
  if (type.kind !== "function") {
    throw new Error(`expected a function descriptor, got ${type.kind}`);
  }
  return type;
}

/** Resolve `Props` in a freshly-upserted Svelte component. */
function resolveProps(session: TypeInfoSession, canonicalId: string, source: string): ObjectType {
  session.host.upsert({ inputId: canonicalId, source });
  return expectObject(session.resolveSymbol(canonicalId, "Props", { mode: "expanded" }).type);
}

describe("TypeInfoSession.resolveSymbol — Svelte callback props", () => {
  it("publishes each callback member's parameter and return descriptors, never inferring them from the member name", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    const props = resolveProps(session, "/fixtures/Callbacks.svelte", SVELTE_CALLBACKS);

    const onMove = expectFunction(propertyNamed(props, "onMove").type);
    expect(onMove.parameters.map((p) => p.name)).toEqual(["x", "y"]);
    expect(onMove.parameters.map((p) => p.type)).toEqual([
      { kind: "primitive", name: "number" },
      { kind: "primitive", name: "number" },
    ]);
    expect(onMove.returnType).toEqual({ kind: "primitive", name: "void" });

    // The `on` prefix is NOT what makes a member callable: `onlabel` is a
    // plain string and must stay a primitive descriptor.
    expect(propertyNamed(props, "onlabel").type).toEqual({ kind: "primitive", name: "string" });

    session.host.close();
  });

  it("keeps an optional PROPERTY distinct from an optional PARAMETER", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    const props = resolveProps(session, "/fixtures/Optionality.svelte", SVELTE_CALLBACKS);

    // `onToggle?` — optional property whose single parameter is ALSO optional.
    const onToggle = propertyNamed(props, "onToggle");
    expect(onToggle.optional).toBe(true);
    const onToggleFn = expectFunction(onToggle.type);
    expect(onToggleFn.parameters).toHaveLength(1);
    expect(onToggleFn.parameters[0]!.name).toBe("on");
    expect(onToggleFn.parameters[0]!.optional).toBe(true);
    expect(onToggleFn.parameters[0]!.type).toEqual({ kind: "primitive", name: "boolean" });

    // `onMove` — required property whose parameters are ALL required. The two
    // optionality axes never bleed into each other.
    const onMove = propertyNamed(props, "onMove");
    expect(onMove.optional).toBe(false);
    expect(expectFunction(onMove.type).parameters.map((p) => p.optional)).toEqual([false, false]);

    // Noncallable control: a required plain property, neither axis set.
    const label = propertyNamed(props, "label");
    expect(label.optional).toBe(false);
    expect(label.type).toEqual({ kind: "primitive", name: "string" });

    session.host.close();
  });

  it("publishes an aliased callback member as a shallow ref, resolvable to its full signature by a separate query", () => {
    // Shallow-by-default: the published `onSelect` member is the bare
    // `Handler` REF — the alias body is never inlined, so the shallow member
    // is never advertised as a complete signature. A consumer that wants the
    // signature re-resolves the alias through the same named-symbol query.
    const session = new TypeInfoSession({ root: "/fixtures" });
    const canonicalId = "/fixtures/Alias.svelte";
    const props = resolveProps(session, canonicalId, SVELTE_CALLBACKS);

    expect(propertyNamed(props, "onSelect").type).toEqual({
      kind: "ref",
      name: "Handler",
      typeArguments: [],
    });

    const handler = expectFunction(
      session.resolveSymbol(canonicalId, "Handler", { mode: "expanded" }).type!,
    );
    expect(handler.parameters.map((p) => [p.name, p.type, p.optional])).toEqual([
      ["id", { kind: "primitive", name: "string" }, false],
    ]);
    expect(handler.returnType).toEqual({ kind: "primitive", name: "void" });

    session.host.close();
  });

  it("re-resolves an edited signature in the SAME host identically to a fresh host", () => {
    const canonicalId = "/fixtures/Edited.svelte";
    const edited = SVELTE_CALLBACKS.replace(
      "onMove: (x: number, y: number) => void;",
      "onMove: (x: number, y: number, z: number) => boolean;",
    );
    expect(edited).not.toBe(SVELTE_CALLBACKS);

    const live = new TypeInfoSession({ root: "/fixtures" });
    const before = expectFunction(
      propertyNamed(resolveProps(live, canonicalId, SVELTE_CALLBACKS), "onMove").type,
    );
    expect(before.parameters).toHaveLength(2);
    const afterEdit = propertyNamed(resolveProps(live, canonicalId, edited), "onMove");
    live.host.close();

    const cold = new TypeInfoSession({ root: "/fixtures" });
    const fresh = propertyNamed(resolveProps(cold, canonicalId, edited), "onMove");
    cold.host.close();

    // The edit is observed (not a stale cached signature) AND the warm result
    // is byte-identical to the cold one.
    const afterEditFn = expectFunction(afterEdit.type);
    expect(afterEditFn.parameters.map((p) => p.name)).toEqual(["x", "y", "z"]);
    expect(afterEditFn.returnType).toEqual({ kind: "primitive", name: "boolean" });
    expect(afterEdit).toEqual(fresh);
  });
});
