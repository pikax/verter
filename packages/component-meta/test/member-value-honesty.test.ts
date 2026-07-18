/**
 * Member-value honesty at the NATIVE binding level.
 *
 * An imported props interface member whose value is a function
 * (`onClick: () => void`) or an inline object (`config: { nested: number }`)
 * publishes its REAL shallow structure: the normalized prop row's
 * member-value source is the authority, and a structural value replays
 * through the projected member-path route on demand. Neither class
 * surfaces a typed failure, a fabricated `unknown`, or a semantic miss.
 */

import { describe, test, expect, afterAll } from "vitest";
import { join } from "path";
import { createCheckerByJson } from "../src/compat/checker.js";
import { shutdownMetaRuntime } from "../src/runtime/index.js";

const fixtureDir = join(__dirname, "fixtures");

afterAll(() => {
  shutdownMetaRuntime();
});

async function getChecker() {
  return createCheckerByJson(fixtureDir, {
    compilerOptions: { strict: true },
    include: ["**/*.vue", "**/*.ts"],
  });
}

describe("member value honesty (native)", () => {
  test("an imported function-valued prop publishes the shallow function structure", async () => {
    const checker = await getChecker();
    const meta = await checker.getComponentMeta(join(fixtureDir, "MemberValueProps.vue"));
    // The typed native payload carries the REAL shallow function structure —
    // never a failure and never a fabricated `unknown`.
    const onClick = meta._verter!.props.find((prop) => prop.name === "onClick");
    expect(onClick, "the imported function-valued prop publishes").toBeDefined();
    expect(onClick!.type).toEqual({
      kind: "function",
      parameters: [],
      returnType: { kind: "primitive", name: "void" },
    });
    expect(JSON.stringify(onClick!.type)).not.toContain("unknown");
    expect(JSON.stringify(onClick!.type)).not.toContain("semanticMiss");
    // The compat display surface renders the same real structure.
    const compat = meta.props.find((prop) => prop.name === "onClick");
    expect(compat?.type).toBe("() => void");
  });

  test("an imported object-valued prop publishes the shallow object structure and its nested leaf", async () => {
    const checker = await getChecker();
    const meta = await checker.getComponentMeta(join(fixtureDir, "MemberValueProps.vue"));
    // The typed native payload carries the REAL shallow object structure
    // whose nested member reaches its declared leaf — never an `unknown`
    // stand-in.
    const config = meta._verter!.props.find((prop) => prop.name === "config");
    expect(config, "the imported object-valued prop publishes").toBeDefined();
    expect(config!.type).toEqual({
      kind: "object",
      properties: [
        {
          name: "nested",
          type: { kind: "primitive", name: "number" },
          optional: false,
        },
      ],
    });
    // The compat display surface renders the same real structure.
    const compat = meta.props.find((prop) => prop.name === "config");
    expect(compat?.type).toBe("{ nested: number }");
  });
});
