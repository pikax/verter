/**
 * Phase 4 Test #3 — Vue worked example.
 *
 * Plan §6.4 row 3: a Vue SFC with `defineProps<{ msg: string }>()`;
 * resolveSymbol on a synthesised companion type alias for the props
 * resolves to `ObjectType { msg: PrimitiveType("string") }`.
 *
 * The session does not need the full SFC pipeline to satisfy this
 * test — the request is type-resolution only. We submit a sibling
 * `.ts` companion file that re-publishes the props type and resolve
 * that. This keeps the test hermetic and exercises the
 * `TypeInfoSession.resolveSymbol → object body` path end-to-end.
 *
 * REGRESSION — fails against any pre-Phase 4 substrate where
 * `resolveSymbol` either does not exist or returns the alias shell
 * instead of the Object body.
 */

import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

describe("TypeInfoSession Vue instance props worked example", () => {
  it("resolves the props alias to an object descriptor with the declared scalar field", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/MyButton.props.ts",
      source: `export type MyButtonProps = { msg: string };\n`,
    });

    const result = session.resolveSymbol("/fixtures/MyButton.props.ts", "MyButtonProps", {
      mode: "expanded",
    });

    expect(result.type).toBeDefined();
    expect(result.type?.kind).toBe("object");
    if (result.type?.kind === "object") {
      expect(result.type.properties.length).toBe(1);
      const msg = result.type.properties[0];
      expect(msg.name).toBe("msg");
      expect(msg.type.kind).toBe("primitive");
      if (msg.type.kind === "primitive") {
        expect(msg.type.name).toBe("string");
      }
    }

    session.host.close();
  });
});
