import { describe, expect, it } from "vitest";

import { isVirtualCarrierPath } from "./virtualCarrier";

describe("virtual carrier path detection", () => {
  it.each([
    "Comp.vue.tsx",
    "Comp.svelte.jsx",
    "Comp.vue.verter.ts",
    "Comp.svelte.__verter_test.ts",
    "Comp.vue.d.ts",
    "Comp.d.vue.ts",
    "Comp.d.svelte.ts",
  ])("rejects %s", (file) => {
    expect(isVirtualCarrierPath(file)).toBe(true);
  });

  it.each(["Comp.vue", "Comp.svelte", "Comp.ts", "Comp.vue.test.ts", "d.vue.ts"])(
    "accepts authored path %s",
    (file) => {
      expect(isVirtualCarrierPath(file)).toBe(false);
    },
  );
});
