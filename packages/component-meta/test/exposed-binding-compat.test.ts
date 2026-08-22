/**
 * `defineExpose` binding admission at the COMPAT surface.
 *
 * Resolves correctly through the native path; this proves the same type
 * reaches `@verter/component-meta/compat` (the `create()`-compatible
 * checker most consumers use), not just the native `_verter` payload.
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

describe("exposed binding compat", () => {
  test("a plain function declaration's defineExpose binding publishes a function type", async () => {
    const checker = await getChecker();
    const meta = await checker.getComponentMeta(join(fixtureDir, "FnExposeCompat.vue"));
    const increment = meta.exposed.find((entry) => entry.name === "increment");
    expect(increment, "increment must be exposed").toBeDefined();
    expect(increment!.type).toBe("function");
  });
});
