import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  loadGeneratedComponentRegistry,
  resolveComponentFile,
} from "./trace-component-resolver.js";

const uiRoot = resolve(import.meta.dirname, "../../../.integration-tests/repos/nuxt-ui");

describe("loadGeneratedComponentRegistry", () => {
  it("maps generated prose component names to nested source files", () => {
    const registry = loadGeneratedComponentRegistry(uiRoot);

    expect(registry.get("ProseCaution")?.replace(/\\/g, "/")).toContain(
      "/src/runtime/components/prose/callout/Caution.vue",
    );
  });
});

describe("resolveComponentFile", () => {
  it("resolves prose subdirectory components from their docs token", () => {
    const file = resolveComponentFile("Caution", { uiRoot });

    expect(file.replace(/\\/g, "/")).toContain("/src/runtime/components/prose/callout/Caution.vue");
  });

  it("resolves explicit generated prose component names", () => {
    const file = resolveComponentFile("ProseCaution", { uiRoot });

    expect(file.replace(/\\/g, "/")).toContain("/src/runtime/components/prose/callout/Caution.vue");
  });
});
