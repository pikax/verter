/**
 * Masked same-name intersection honesty at the NATIVE binding level.
 *
 * A `defineProps<{ x: string } & Bad>()` whose imported same-name
 * contributor `Bad.x` references a non-existent type must FAIL the typed
 * native query — the resolvable local `x: string` arm must never mask the
 * failed contributor into a completed `{ kind: "primitive", name: "string" }`
 * success. Both intersection orders fail identically, matching the plain
 * `defineProps<Bad>()` genuine miss: the RequiredSourceUnavailable output
 * error renders as the typed "REQUIRED member-value position has no
 * representable source" materialization failure at the prop's type lane.
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

describe("masked same-name intersection honesty (native)", () => {
  test("a failed imported same-name contributor fails the query typed (local arm first)", async () => {
    const checker = await getChecker();
    await expect(
      checker.getComponentMeta(join(fixtureDir, "MaskedIntersectionProps.vue")),
    ).rejects.toThrow(
      /output materialization failed at props\[\]\.type.*REQUIRED member-value position has no representable source/,
    );
  });

  test("a failed imported same-name contributor fails the query typed (imported arm first)", async () => {
    const checker = await getChecker();
    await expect(
      checker.getComponentMeta(join(fixtureDir, "MaskedIntersectionPropsReversed.vue")),
    ).rejects.toThrow(
      /output materialization failed at props\[\]\.type.*REQUIRED member-value position has no representable source/,
    );
  });
});
